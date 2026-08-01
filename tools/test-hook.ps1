param(
    [Parameter(Mandatory = $true)]
    [string] $TargetDir
)

$ErrorActionPreference = "Stop"
$Harness = Join-Path $TargetDir "hook-harness.exe"

if (-not (Test-Path -PathType Leaf $Harness)) {
    throw "Required hook harness is missing: $Harness"
}

$Output = @(& $Harness)
if ($LASTEXITCODE -ne 0) {
    throw "hook harness exited with code $LASTEXITCODE"
}
if ($Output.Count -ne 1) {
    throw "hook harness emitted $($Output.Count) stdout lines, expected one"
}
if ($Output[0] -notmatch '^hook harness passed: relocated_bytes=\d+ observations=\d+ concurrent_calls=\d+$') {
    throw "hook harness emitted an unexpected result: $($Output[0])"
}

Write-Host $Output[0]
