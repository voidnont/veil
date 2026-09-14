param(
    [string]$SourceDir
)

$ErrorActionPreference = "Stop"
$RepoRoot = Split-Path -Parent $PSScriptRoot
if (-not $SourceDir) {
    $SourceDir = Join-Path $RepoRoot ".gecko-src"
}

$Python = Get-Command python.exe -ErrorAction SilentlyContinue
if (-not $Python) {
    $Python = Get-Command python3.exe -ErrorAction SilentlyContinue
}
if (-not $Python) {
    throw "Python 3 is required to verify the Gecko manifest."
}

$Verifier = Join-Path $RepoRoot "tools\gecko_manifest.py"
& $Python.Source $Verifier --repo-root $RepoRoot --source $SourceDir
if ($LASTEXITCODE -ne 0) {
    exit $LASTEXITCODE
}
