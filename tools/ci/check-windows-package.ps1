# Synthetic first launch only. Never reuse an installed app or the user's profile.
$ErrorActionPreference = 'Stop'
$root = Split-Path (Split-Path $PSScriptRoot -Parent) -Parent
$installers = @(Get-ChildItem -Path "$root/dist/Patina-*-windows-x86_64-setup.exe")
if ($installers.Count -ne 1) { throw 'Expected exactly one Windows release installer.' }
$registration = 'HKCU:/Software/Microsoft/Windows/CurrentVersion/Uninstall/com.hantekllc.iptvrs_is1'
if (Test-Path -LiteralPath $registration) { throw 'Refusing to smoke-test over an existing Patina installation.' }
$scratch = Join-Path $root "target/windows-install-smoke/$([guid]::NewGuid())"
$installed = Join-Path $scratch 'app'
$profile = Join-Path $scratch 'profile'
New-Item -ItemType Directory -Force $profile | Out-Null
$previousProfile = $env:LOCALAPPDATA
$app = $null
if (!("PatinaPackageWindow" -as [type])) {
    Add-Type @"
using System;
using System.Text;
using System.Runtime.InteropServices;
public static class PatinaPackageWindow {
    private delegate bool Visitor(IntPtr window, IntPtr state);
    [DllImport("user32.dll")] private static extern bool EnumWindows(Visitor visitor, IntPtr state);
    [DllImport("user32.dll")] private static extern uint GetWindowThreadProcessId(IntPtr window, out uint process);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] private static extern int GetClassName(IntPtr window, StringBuilder text, int count);
    [DllImport("user32.dll")] public static extern IntPtr GetClassLongPtrW(IntPtr window, int index);
    [DllImport("user32.dll")] public static extern bool PostMessageW(IntPtr window, uint message, IntPtr wparam, IntPtr lparam);
    public static IntPtr Find(uint process) {
        IntPtr found = IntPtr.Zero;
        EnumWindows((window, state) => {
            uint owner;
            GetWindowThreadProcessId(window, out owner);
            if (owner == process) {
                var name = new StringBuilder(128);
                GetClassName(window, name, name.Capacity);
                if (name.ToString() == "NshWindow") { found = window; return false; }
            }
            return true;
        }, IntPtr.Zero);
        return found;
    }
}
"@
}
try {
    $setup = Start-Process -FilePath $installers[0].FullName -ArgumentList @('/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART', '/NOICONS', "/DIR=`"$installed`"") -WindowStyle Hidden -Wait -PassThru
    if ($setup.ExitCode -ne 0) { throw "Installer failed: $($setup.ExitCode)" }
    foreach ($file in @('Patina.exe', 'libmpv-2.dll', 'vcruntime140.dll', 'vcruntime140_1.dll', 'LICENSE', 'notices/THIRD-PARTY.html', 'notices/components.json', 'notices/libmpv/licenses', 'notices/libmpv/components.json', 'notices/libmpv/sources.lock.json', 'notices/libmpv/SOURCE.txt', 'notices/libmpv/NOTICE.txt', 'notices/libmpv/build-config')) {
        if (!(Test-Path -LiteralPath (Join-Path $installed $file))) { throw "Installed file missing: $file" }
    }
    $runtimePin = Get-Content -LiteralPath "$root/tools/mpv-runtime/runtime.json" -Raw | ConvertFrom-Json
    $runtimeInventory = Get-Content -LiteralPath (Join-Path $installed 'notices/libmpv/components.json') -Raw | ConvertFrom-Json
    $dllHash = (Get-FileHash -LiteralPath (Join-Path $installed 'libmpv-2.dll') -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($dllHash -ne $runtimePin.dll_sha256 -or $dllHash -ne $runtimeInventory.build.dll_sha256) { throw 'Installed libmpv does not match pinned source inventory.' }
    & python "$PSScriptRoot/check-windows-icon.py" (Join-Path $installed 'Patina.exe')
    if ($LASTEXITCODE -ne 0) { throw 'Installed icon is missing.' }
    $env:LOCALAPPDATA = $profile
    $app = Start-Process -FilePath (Join-Path $installed 'Patina.exe') -WorkingDirectory $installed -WindowStyle Hidden -PassThru
    $deadline = [DateTime]::UtcNow.AddSeconds(20)
    do {
        Start-Sleep -Milliseconds 250
        $app.Refresh()
        if ($app.HasExited) { throw "Installed app exited during startup: $($app.ExitCode)" }
        $window = [PatinaPackageWindow]::Find($app.Id)
    } while ($window -eq [IntPtr]::Zero -and [DateTime]::UtcNow -lt $deadline)
    if ($window -eq [IntPtr]::Zero) { throw 'Installed app did not create a window.' }
    if ([PatinaPackageWindow]::GetClassLongPtrW($window, -14) -eq [IntPtr]::Zero) { throw 'App window has no icon.' }
    if (![PatinaPackageWindow]::PostMessageW($window, 0x10, [IntPtr]::Zero, [IntPtr]::Zero) -or !$app.WaitForExit(10000)) { throw 'Installed app did not close cleanly.' }
    if ($app.ExitCode -ne 0) { throw "Installed app exit code: $($app.ExitCode)" }
    # A sentinel verifies uninstall never removes the application's data directory.
    $data = Join-Path $profile 'Patina'
    New-Item -ItemType Directory -Force $data | Out-Null
    Set-Content -LiteralPath (Join-Path $data 'synthetic-settings.txt') -Value 'synthetic settings'
} finally {
    $env:LOCALAPPDATA = $previousProfile
    if ($app -and !$app.HasExited) { $app.Kill(); $app.WaitForExit() }
    $uninstaller = Join-Path $installed 'unins000.exe'
    if (Test-Path -LiteralPath $uninstaller) {
        $remove = Start-Process -FilePath $uninstaller -ArgumentList @('/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART') -WindowStyle Hidden -Wait -PassThru
        if ($remove.ExitCode -ne 0) { throw "Uninstall failed: $($remove.ExitCode)" }
    }
}
if (Test-Path -LiteralPath (Join-Path $installed 'Patina.exe')) { throw 'Uninstall left the app binary.' }
if (!(Test-Path -LiteralPath (Join-Path $profile 'Patina/synthetic-settings.txt'))) { throw 'Uninstall removed user data.' }
if (Test-Path -LiteralPath $registration) { throw 'Uninstall left its registration.' }
Write-Host 'Windows installer: installation, icon, launch, clean exit and uninstall passed.'
