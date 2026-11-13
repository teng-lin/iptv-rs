# Pin the compiler independently of the hosted runner's preinstalled version.
$ErrorActionPreference = 'Stop'
$root = Split-Path (Split-Path $PSScriptRoot -Parent) -Parent
$tools = Join-Path $root 'target/tools'
$destination = Join-Path $tools 'inno-7.1.1'
$compiler = Join-Path $destination 'ISCC.exe'
if (!(Test-Path -LiteralPath $compiler)) {
    New-Item -ItemType Directory -Force $tools | Out-Null
    $installer = Join-Path $tools 'innosetup-7.1.1-x64.exe'
    $hash = '8a5a6711c3766a6e5fb74f280ba0e3551e719ce94538d901fdbd1f8f9920b3f0'
    if (!(Test-Path -LiteralPath $installer)) {
        Invoke-WebRequest 'https://github.com/jrsoftware/issrc/releases/download/is-7_1_1/innosetup-7.1.1-x64.exe' -OutFile $installer
    }
    if ((Get-FileHash -LiteralPath $installer -Algorithm SHA256).Hash -ne $hash) { throw 'Inno Setup checksum mismatch.' }
    $process = Start-Process -FilePath $installer -ArgumentList @('/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART', '/CURRENTUSER', '/NOICONS', "/DIR=`"$destination`"") -WindowStyle Hidden -Wait -PassThru
    if ($process.ExitCode -ne 0) { throw "Inno Setup installation failed: $($process.ExitCode)" }
}
if (!(Test-Path -LiteralPath $compiler)) { throw 'Inno Setup compiler is missing.' }
$compiler
