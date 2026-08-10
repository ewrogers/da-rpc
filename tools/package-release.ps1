[CmdletBinding()]
param(
    [Parameter(Mandatory)]
    [ValidatePattern('^v\d+\.\d+\.\d+$')]
    [string] $Tag,

    [Parameter(Mandatory)]
    [string] $X86TargetDir,

    [Parameter(Mandatory)]
    [string] $X64TargetDir,

    [string] $OutputDir = "dist"
)

$ErrorActionPreference = "Stop"

function Get-PeMachine {
    param(
        [Parameter(Mandatory)]
        [string] $Path
    )

    $bytes = [IO.File]::ReadAllBytes($Path)
    if ($bytes.Length -lt 0x40 -or $bytes[0] -ne 0x4d -or $bytes[1] -ne 0x5a) {
        throw "not a Portable Executable file: $Path"
    }

    $headerOffset = [BitConverter]::ToInt32($bytes, 0x3c)
    if ($headerOffset -lt 0 -or $headerOffset + 6 -gt $bytes.Length) {
        throw "invalid Portable Executable header offset: $Path"
    }
    if ([BitConverter]::ToUInt32($bytes, $headerOffset) -ne 0x00004550) {
        throw "invalid Portable Executable signature: $Path"
    }

    [BitConverter]::ToUInt16($bytes, $headerOffset + 4)
}

$sourceRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot ".."))
$x86TargetPath = (Resolve-Path -LiteralPath $X86TargetDir).Path
$x64TargetPath = (Resolve-Path -LiteralPath $X64TargetDir).Path
$outputPath = if ([IO.Path]::IsPathRooted($OutputDir)) {
    [IO.Path]::GetFullPath($OutputDir)
} else {
    [IO.Path]::GetFullPath((Join-Path (Get-Location) $OutputDir))
}

$artifacts = @(
    @{ Name = "darpc.dll"; Source = (Join-Path $x86TargetPath "darpc.dll"); Machine = 0x014c },
    @{ Name = "loader.exe"; Source = (Join-Path $x86TargetPath "loader.exe"); Machine = 0x014c },
    @{ Name = "darpc.exe"; Source = (Join-Path $x64TargetPath "darpc.exe"); Machine = 0x8664 },
    @{ Name = "darpcd.exe"; Source = (Join-Path $x64TargetPath "darpcd.exe"); Machine = 0x8664 }
)

foreach ($artifact in $artifacts) {
    if (-not (Test-Path -LiteralPath $artifact.Source -PathType Leaf)) {
        throw "missing release artifact: $($artifact.Source)"
    }
    $machine = Get-PeMachine -Path $artifact.Source
    if ($machine -ne $artifact.Machine) {
        throw ("wrong architecture for {0}: expected 0x{1:x4}, found 0x{2:x4}" -f `
                $artifact.Name, $artifact.Machine, $machine)
    }
}

$bundle = Join-Path $outputPath "da-rpc-$Tag-windows"
$archive = "$bundle.zip"
$checksum = "$archive.sha256"
foreach ($path in @($bundle, $archive, $checksum)) {
    if (Test-Path -LiteralPath $path) {
        throw "release output already exists: $path"
    }
}

New-Item -ItemType Directory -Path $bundle -Force | Out-Null
foreach ($artifact in $artifacts) {
    Copy-Item -LiteralPath $artifact.Source -Destination (Join-Path $bundle $artifact.Name)
}
Copy-Item -LiteralPath (Join-Path $sourceRoot "README.md") -Destination $bundle
Copy-Item -LiteralPath (Join-Path $sourceRoot "LICENSE") -Destination $bundle

Get-ChildItem -LiteralPath $bundle -File |
    Sort-Object Name |
    ForEach-Object {
        $hash = (Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash.ToLowerInvariant()
        "$hash  $($_.Name)"
    } |
    Set-Content -LiteralPath (Join-Path $bundle "SHA256SUMS") -Encoding ascii

tar.exe -a -c -f $archive -C $bundle .
if ($LASTEXITCODE -ne 0) {
    throw "failed to create release archive"
}

$archiveHash = (Get-FileHash -LiteralPath $archive -Algorithm SHA256).Hash.ToLowerInvariant()
$archiveName = Split-Path $archive -Leaf
"$archiveHash  $archiveName" |
    Set-Content -LiteralPath $checksum -Encoding ascii -NoNewline

Write-Output "Created $archive"
Write-Output "Created $checksum"
