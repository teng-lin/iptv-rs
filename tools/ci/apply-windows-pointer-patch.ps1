[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string]$Repository,
    [Parameter(Mandatory = $true)][string]$Patch
)
$ErrorActionPreference = 'Stop'
# A reverse check normally fails on a fresh checkout. PowerShell 5.1 turns
# redirected native stderr into terminating errors when the preference is Stop.
$previousPreference = $ErrorActionPreference
try {
    $ErrorActionPreference = 'Continue'
    & git -C $Repository apply --ignore-space-change --reverse --check $Patch 2>$null
    $alreadyApplied = $LASTEXITCODE -eq 0
} finally {
    $ErrorActionPreference = $previousPreference
}
if (!$alreadyApplied) {
    # Git for Windows may check out Rust sources with CRLF. Match context
    # across line endings while retaining every substantive patch check.
    & git -C $Repository apply --ignore-space-change --check $Patch
    if ($LASTEXITCODE -ne 0) { throw 'NSH pointer patch does not match this checkout.' }
    & git -C $Repository apply --ignore-space-change $Patch
    if ($LASTEXITCODE -ne 0) { throw 'Could not apply NSH pointer fix.' }
}
