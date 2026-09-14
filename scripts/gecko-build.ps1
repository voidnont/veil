param(
    [string]$SourceDir
)

$ErrorActionPreference = "Stop"
$RepoRoot = Split-Path -Parent $PSScriptRoot
if (-not $SourceDir) {
    $SourceDir = Join-Path $RepoRoot ".gecko-src"
}

if (-not (Test-Path (Join-Path $SourceDir ".git"))) {
    throw "Gecko checkout not found at $SourceDir; run scripts/gecko-bootstrap.ps1 first."
}

$MozillaBuildBin = "C:\mozilla-build\bin"
if (Test-Path $MozillaBuildBin) {
    $env:PATH = "$MozillaBuildBin;$env:PATH"
}

$Python = Get-Command python.exe -ErrorAction SilentlyContinue
if (-not $Python) {
    $Python = Get-Command python3.exe -ErrorAction SilentlyContinue
}
if (-not $Python) {
    throw "Python 3 is required to build Gecko."
}

$Verifier = Join-Path $RepoRoot "tools\gecko_manifest.py"
& $Python.Source $Verifier --repo-root $RepoRoot --source $SourceDir --allow-dirty
if ($LASTEXITCODE -ne 0) {
    exit $LASTEXITCODE
}

Copy-Item (Join-Path $RepoRoot "gecko\mozconfig.windows") (Join-Path $SourceDir ".mozconfig") -Force

$Mach = Join-Path $SourceDir "mach.ps1"
if (-not (Test-Path $Mach)) {
    throw "mach.ps1 was not found in the pinned Gecko checkout."
}

Set-ExecutionPolicy -Scope Process -ExecutionPolicy Bypass -Force
Push-Location $SourceDir
try {
    & $Mach "--no-interactive" "bootstrap" "--application-choice=Firefox for Desktop"
    if ($LASTEXITCODE -ne 0) { throw "mach bootstrap failed with exit code $LASTEXITCODE" }

    & $Mach "build"
    if ($LASTEXITCODE -ne 0) { throw "mach build failed with exit code $LASTEXITCODE" }

    & $Mach "package"
    if ($LASTEXITCODE -ne 0) { throw "mach package failed with exit code $LASTEXITCODE" }
} finally {
    Pop-Location
}
