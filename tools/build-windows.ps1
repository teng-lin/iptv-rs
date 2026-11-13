[CmdletBinding()]
param([switch]$Check, [switch]$Release, [switch]$Run)
$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
Push-Location $root
try {
    $env:PATH = "$env:USERPROFILE\.cargo\bin;$env:PATH"
    $vswhere = "${env:ProgramFiles(x86)}\Microsoft Visual Studio\Installer\vswhere.exe"
    if (!(Test-Path $vswhere)) { throw 'Install Visual Studio Build Tools with Desktop development with C++.' }
    $vs = & $vswhere -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
    if (!$vs) { throw 'The Visual Studio C++ workload is missing.' }
    # Cargo and cc discover the installed MSVC/SDK through Visual Studio's registry.
    # Keep the environment consistent with ordinary cargo commands to reuse native builds.
    if (Test-Path 'C:/Strawberry/perl/bin/perl.exe') {
        $env:PATH = "C:\Strawberry\perl\bin;C:\Strawberry\c\bin;$env:PATH"
    }
    if (!(Get-Command perl -ErrorAction SilentlyContinue)) {
        throw 'Install Strawberry Perl and put its perl/bin directory on PATH (SQLCipher builds OpenSSL).'
    }
    # NSH fix carried locally until upstreamed: drag-region crossings retain hover.
    $pointerPatch = Join-Path $PSScriptRoot 'patches/nsh-windows-pointer-leave.patch'
    & git -C ../nsh apply --reverse --check $pointerPatch 2>$null
    if ($LASTEXITCODE -ne 0) {
        & git -C ../nsh apply --check $pointerPatch
        if ($LASTEXITCODE -ne 0) { throw 'NSH pointer patch does not match this checkout.' }
        & git -C ../nsh apply $pointerPatch
        if ($LASTEXITCODE -ne 0) { throw 'Could not apply NSH pointer fix.' }
    }
    & "$PSScriptRoot/fetch-mpv-windows.ps1"
    $env:PATH = "$(Join-Path $root 'target/mpv');$env:PATH"
    if ($Check) {
        & cargo clippy --locked --workspace --all-targets --features iptv-app/test-support -- -D warnings
        if ($LASTEXITCODE -ne 0) { throw 'Windows lint checks failed' }
        & cargo test --locked -p nsh-platform-windows --lib chrome::pointer_tests
        if ($LASTEXITCODE -ne 0) { throw 'Windows pointer regression failed' }
        & cargo test --locked -p iptv-import -p iptv-providers -p iptv-net
        if ($LASTEXITCODE -ne 0) { throw 'Import, provider or transport tests failed' }
        & cargo test --locked -p iptv-core
        if ($LASTEXITCODE -ne 0) { throw 'Core tests failed' }
        & cargo build --locked -p iptv-core --target wasm32-unknown-unknown
        if ($LASTEXITCODE -ne 0) { throw 'Core wasm build failed' }
        & cargo test --locked -p iptv-store-sqlite
        if ($LASTEXITCODE -ne 0) { throw 'Encrypted store tests failed' }
        & cargo test --locked -p iptv-app --lib --features test-support
        if ($LASTEXITCODE -ne 0) { throw 'App unit tests failed' }
        & cargo test --locked -p iptv-app --test app --features test-support
        if ($LASTEXITCODE -ne 0) { throw 'App scenarios failed' }
        & cargo test --locked -p iptv-app --test app --features test-support store_latency::guide_input_stays_under_budget_while_the_database_writer_is_locked -- --ignored --exact
        if ($LASTEXITCODE -ne 0) { throw 'Guide latency check failed' }
    }
    $buildArgs = @('build', '--locked', '-p', 'iptv-app')
    $profile = 'debug'
    if ($Release) { $buildArgs += '--release'; $profile = 'release' }
    & cargo @buildArgs
    if ($LASTEXITCODE -ne 0) { throw 'Windows app build failed' }
    $output = Join-Path $root "target/$profile"
    Copy-Item -LiteralPath (Join-Path $root 'target/mpv/libmpv-2.dll') -Destination $output
    Write-Host "Built $output/iptv-app.exe"
    if ($Run) { & (Join-Path $output 'iptv-app.exe') }
} finally { Pop-Location }
