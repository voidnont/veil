param(
    [string]$SourceDir
)

$ErrorActionPreference = "Stop"
$RepoRoot = Split-Path -Parent $PSScriptRoot
if (-not $SourceDir) {
    $SourceDir = Join-Path $RepoRoot ".gecko-src"
}

$UpstreamUrl = "https://github.com/mozilla-firefox/firefox.git"
$RevisionFile = Join-Path $RepoRoot "gecko\REVISION"
$SeriesFile = Join-Path $RepoRoot "gecko\patches\series"
$Verifier = Join-Path $RepoRoot "tools\gecko_manifest.py"
$Branding = Join-Path $RepoRoot "tools\gecko_branding.py"
$Preferences = Join-Path $RepoRoot "tools\gecko_prefs.py"

$Python = Get-Command python.exe -ErrorAction SilentlyContinue
if (-not $Python) {
    $Python = Get-Command python3.exe -ErrorAction SilentlyContinue
}
if (-not $Python) {
    throw "Python 3 is required to bootstrap Gecko."
}

function Invoke-Checked {
    param(
        [scriptblock]$Command,
        [string]$Description
    )
    & $Command
    if ($LASTEXITCODE -ne 0) {
        throw "$Description failed with exit code $LASTEXITCODE"
    }
}

& $Python.Source $Verifier --repo-root $RepoRoot --manifest-only
if ($LASTEXITCODE -ne 0) {
    exit $LASTEXITCODE
}

$Revision = (Get-Content -Raw $RevisionFile).Trim()

if ((Test-Path $SourceDir) -and -not (Test-Path (Join-Path $SourceDir ".git"))) {
    throw "$SourceDir exists but is not a Git checkout."
}
if (-not (Test-Path $SourceDir)) {
    New-Item -ItemType Directory -Path $SourceDir -Force | Out-Null
}
if (-not (Test-Path (Join-Path $SourceDir ".git"))) {
    Invoke-Checked { git -C $SourceDir init } "git init"
}

Invoke-Checked { git -C $SourceDir config core.longpaths true } "git config"
$Remotes = @(git -C $SourceDir remote)
if ($LASTEXITCODE -ne 0) {
    throw "git remote failed with exit code $LASTEXITCODE"
}
if ($Remotes -contains "origin") {
    Invoke-Checked { git -C $SourceDir remote set-url origin $UpstreamUrl } "git remote set-url"
} else {
    Invoke-Checked { git -C $SourceDir remote add origin $UpstreamUrl } "git remote add"
}

Invoke-Checked { git -C $SourceDir fetch --depth=1 origin $Revision } "git fetch pinned REVISION"
Invoke-Checked { git -C $SourceDir checkout --detach --force $Revision } "git checkout pinned REVISION"
Invoke-Checked { git -C $SourceDir reset --hard $Revision } "git reset pinned REVISION"
Invoke-Checked { git -C $SourceDir clean -ffdx } "git clean"

& $Python.Source $Verifier --repo-root $RepoRoot --source $SourceDir
if ($LASTEXITCODE -ne 0) {
    exit $LASTEXITCODE
}

foreach ($RawLine in Get-Content $SeriesFile) {
    $Line = $RawLine.Trim()
    if (-not $Line -or $Line.StartsWith("#")) {
        continue
    }
    $PatchPath = Join-Path $RepoRoot ("gecko\patches\" + $Line)
    Invoke-Checked { git -C $SourceDir apply --whitespace=nowarn $PatchPath } "git apply $Line"
}

& $Python.Source $Branding --repo-root $RepoRoot --source $SourceDir
if ($LASTEXITCODE -ne 0) {
    exit $LASTEXITCODE
}

& $Python.Source $Preferences --repo-root $RepoRoot --source $SourceDir
if ($LASTEXITCODE -ne 0) {
    exit $LASTEXITCODE
}

Write-Host "Prepared Gecko $Revision with Veil branding and privacy defaults in $SourceDir"
