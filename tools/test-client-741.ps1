param(
    [Parameter(Mandatory = $true)]
    [string] $ClientPath,

    [Parameter(Mandatory = $true)]
    [string] $TargetDir
)

$ErrorActionPreference = "Stop"

$Loader = Join-Path $TargetDir "loader.exe"
$Target = Join-Path $TargetDir "injection-target.exe"
$DarpcDll = Join-Path $TargetDir "darpc.dll"

foreach ($Path in @($ClientPath, $Loader, $Target, $DarpcDll)) {
    if (-not (Test-Path -PathType Leaf $Path)) {
        throw "Required file is missing: $Path"
    }
}

$ClientPath = (Resolve-Path $ClientPath).Path
$ClientDirectory = Split-Path -Parent $ClientPath
$Loader = (Resolve-Path $Loader).Path
$Target = (Resolve-Path $Target).Path
$DarpcDll = (Resolve-Path $DarpcDll).Path
$ClientProcessName = [System.IO.Path]::GetFileNameWithoutExtension($ClientPath)
$PreviousClientBypass = $env:DARPC_LOADER_TEST_ALLOW_UNSUPPORTED_CLIENT

function Assert-True {
    param(
        [bool] $Condition,
        [string] $Message
    )

    if (-not $Condition) {
        throw $Message
    }
}

function Invoke-Loader {
    param(
        [string[]] $CommandArgs,
        [int] $ExpectedExitCode = 0
    )

    $PreviousErrorActionPreference = $ErrorActionPreference

    try {
        $ErrorActionPreference = "Continue"
        $Output = @(& $Loader --json @CommandArgs 2>$null)
        $ExitCode = $LASTEXITCODE
    } finally {
        $ErrorActionPreference = $PreviousErrorActionPreference
    }

    if ($ExitCode -ne $ExpectedExitCode) {
        throw "loader exit code was $ExitCode, expected $ExpectedExitCode for: $CommandArgs"
    }

    if ($Output.Count -ne 1) {
        throw "loader emitted $($Output.Count) stdout lines, expected one JSON result"
    }

    return $Output[0] | ConvertFrom-Json
}

function Stop-OwnedProcess {
    param(
        [System.Diagnostics.Process] $Process
    )

    if ($null -ne $Process -and -not $Process.HasExited) {
        Stop-Process -Id $Process.Id -Force
        $Process.WaitForExit()
    }
}

function Assert-ProcessRunning {
    param(
        [System.Diagnostics.Process] $Process,
        [string] $Context
    )

    $Process.Refresh()
    Assert-True (-not $Process.HasExited) "process exited after $Context"
}

function Get-LifecycleLog {
    param(
        [int] $ProcessId
    )

    return Join-Path $env:USERPROFILE "darpc\logs\pid-$ProcessId.log"
}

function Read-LifecycleLog {
    param(
        [string] $LogPath
    )

    $Share = [System.IO.FileShare]::ReadWrite -bor [System.IO.FileShare]::Delete
    $Stream = [System.IO.File]::Open(
        $LogPath,
        [System.IO.FileMode]::Open,
        [System.IO.FileAccess]::Read,
        $Share
    )
    $Reader = [System.IO.StreamReader]::new($Stream)

    try {
        return $Reader.ReadToEnd() -split "`r?`n"
    } finally {
        $Reader.Dispose()
    }
}

function Wait-ForLifecycleEvent {
    param(
        [string] $LogPath,
        [string] $Event,
        [int] $ProcessId,
        [int] $TimeoutMilliseconds = 5000,
        [DateTime] $NotBefore = [DateTime]::MinValue
    )

    $ExpectedPrefix = "event=$Event pid=$ProcessId version="
    $Deadline = [DateTime]::UtcNow.AddMilliseconds($TimeoutMilliseconds)

    while ([DateTime]::UtcNow -lt $Deadline) {
        if (Test-Path -PathType Leaf $LogPath) {
            $Lines = Read-LifecycleLog $LogPath

            $LogTimestamp = (Get-Item $LogPath).LastWriteTimeUtc

            if (
                $LogTimestamp -ge $NotBefore -and
                $null -ne ($Lines | Where-Object { $_.StartsWith($ExpectedPrefix) })
            ) {
                return
            }
        }

        Start-Sleep -Milliseconds 25
    }

    throw "timed out waiting for $Event in lifecycle log: $LogPath"
}

function Assert-NoRunningClient {
    $Running = @(Get-Process -Name $ClientProcessName -ErrorAction SilentlyContinue)

    if ($Running.Count -ne 0) {
        throw "Close all $ClientProcessName processes before running the live-client checks"
    }
}

function Test-LiveLifecycle {
    param(
        [System.Diagnostics.Process] $Process,
        [string] $Context
    )

    $LifecycleLog = Get-LifecycleLog $Process.Id
    Remove-Item $LifecycleLog -Force -ErrorAction SilentlyContinue

    $Result = Invoke-Loader -CommandArgs @("attach", "$($Process.Id)", $DarpcDll)
    Assert-True $Result.changed "$Context attach did not report a state change"
    Assert-True $Result.darpc_loaded "$Context attach did not observe darpc.dll"
    Wait-ForLifecycleEvent $LifecycleLog "initialized" $Process.Id

    $Result = Invoke-Loader `
        -CommandArgs @("attach", "$($Process.Id)", $DarpcDll) `
        -ExpectedExitCode 9
    Assert-True ($Result.error.kind -eq "already_loaded") "$Context duplicate attach was not detected"
    Assert-ProcessRunning $Process "$Context duplicate attach"

    $Result = Invoke-Loader -CommandArgs @("inspect", "$($Process.Id)")
    Assert-True $Result.darpc_loaded "$Context inspect did not observe darpc.dll"

    $Result = Invoke-Loader -CommandArgs @("detach", "$($Process.Id)", $DarpcDll)
    Assert-True $Result.changed "$Context detach did not report a state change"
    Assert-True (-not $Result.darpc_loaded) "$Context detach left darpc.dll loaded"
    Wait-ForLifecycleEvent $LifecycleLog "shutdown" $Process.Id
    Assert-ProcessRunning $Process "$Context detach"
}

try {
    Remove-Item Env:DARPC_LOADER_TEST_ALLOW_UNSUPPORTED_CLIENT -ErrorAction SilentlyContinue

    Write-Host "Testing unsupported executable rejection"
    $Process = Start-Process -FilePath $Target -ArgumentList "--wait-ms", "30000" -PassThru
    try {
        Start-Sleep -Milliseconds 200
        Assert-ProcessRunning $Process "unsupported target startup"

        $Result = Invoke-Loader `
            -CommandArgs @("attach", "$($Process.Id)", $DarpcDll) `
            -ExpectedExitCode 16
        Assert-True ($Result.error.kind -eq "unsupported_client") "unsupported attach error was not distinct"
        Assert-ProcessRunning $Process "unsupported attach"

        $Result = Invoke-Loader -CommandArgs @("inspect", "$($Process.Id)")
        Assert-True (-not $Result.darpc_loaded) "unsupported attach loaded darpc.dll"
    } finally {
        Stop-OwnedProcess $Process
    }

    $BeforeIds = @(Get-Process -Name "injection-target" -ErrorAction SilentlyContinue | ForEach-Object Id)
    $NewProcesses = @()
    try {
        $Result = Invoke-Loader `
            -CommandArgs @("launch", $Target, $DarpcDll, "--", "--wait-ms", "30000") `
            -ExpectedExitCode 16
        Assert-True ($Result.error.kind -eq "unsupported_client") "unsupported launch error was not distinct"

        $After = @(Get-Process -Name "injection-target" -ErrorAction SilentlyContinue)
        $NewProcesses = @($After | Where-Object { $BeforeIds -notcontains $_.Id })
        Assert-True ($NewProcesses.Count -eq 0) "unsupported launch created a child process"
    } finally {
        $After = @(Get-Process -Name "injection-target" -ErrorAction SilentlyContinue)
        $NewProcesses = @($After | Where-Object { $BeforeIds -notcontains $_.Id })

        foreach ($UnexpectedProcess in $NewProcesses) {
            Stop-OwnedProcess $UnexpectedProcess
        }
    }

    Write-Host "Testing separate controlled target processes"
    $env:DARPC_LOADER_TEST_ALLOW_UNSUPPORTED_CLIENT = "1"
    $FirstProcess = $null
    $SecondProcess = $null
    try {
        $FirstProcess = Start-Process -FilePath $Target -ArgumentList "--wait-ms", "30000" -PassThru
        $SecondProcess = Start-Process -FilePath $Target -ArgumentList "--wait-ms", "30000" -PassThru
        Start-Sleep -Milliseconds 200
        Assert-ProcessRunning $FirstProcess "first controlled target startup"
        Assert-ProcessRunning $SecondProcess "second controlled target startup"

        foreach ($ControlledProcess in @($FirstProcess, $SecondProcess)) {
            $Result = Invoke-Loader -CommandArgs @("attach", "$($ControlledProcess.Id)", $DarpcDll)
            Assert-True $Result.darpc_loaded "controlled target did not load darpc.dll"
        }

        foreach ($ControlledProcess in @($FirstProcess, $SecondProcess)) {
            $Result = Invoke-Loader -CommandArgs @("inspect", "$($ControlledProcess.Id)")
            Assert-True $Result.darpc_loaded "controlled target lost its darpc.dll instance"
            $Result = Invoke-Loader -CommandArgs @("detach", "$($ControlledProcess.Id)", $DarpcDll)
            Assert-True (-not $Result.darpc_loaded) "controlled target did not unload darpc.dll"
        }
    } finally {
        Stop-OwnedProcess $FirstProcess
        Stop-OwnedProcess $SecondProcess
        Remove-Item Env:DARPC_LOADER_TEST_ALLOW_UNSUPPORTED_CLIENT -ErrorAction SilentlyContinue
    }

    Write-Host "Testing Dark Ages 7.41 late attach"
    Assert-NoRunningClient
    $Process = Start-Process `
        -FilePath $ClientPath `
        -WorkingDirectory $ClientDirectory `
        -PassThru
    try {
        Start-Sleep -Seconds 4
        Assert-ProcessRunning $Process "live client startup"
        Test-LiveLifecycle $Process "late attach"
    } finally {
        Stop-OwnedProcess $Process
    }

    Write-Host "Testing Dark Ages 7.41 suspended launch"
    Assert-NoRunningClient
    $LaunchStartedAt = [DateTime]::UtcNow.AddSeconds(-1)
    $Process = $null
    try {
        $Result = Invoke-Loader -CommandArgs @("launch", $ClientPath, $DarpcDll)
        $Process = Get-Process -Id $Result.pid -ErrorAction Stop
        Start-Sleep -Seconds 4
        Assert-ProcessRunning $Process "suspended launch startup"
        Assert-True $Result.darpc_loaded "suspended launch did not observe darpc.dll"

        $LifecycleLog = Get-LifecycleLog $Process.Id
        Wait-ForLifecycleEvent `
            -LogPath $LifecycleLog `
            -Event "initialized" `
            -ProcessId $Process.Id `
            -NotBefore $LaunchStartedAt

        $Result = Invoke-Loader -CommandArgs @("inspect", "$($Process.Id)")
        Assert-True $Result.darpc_loaded "suspended launch inspect did not observe darpc.dll"

        $Result = Invoke-Loader -CommandArgs @("detach", "$($Process.Id)", $DarpcDll)
        Assert-True (-not $Result.darpc_loaded) "suspended launch detach left darpc.dll loaded"
        Wait-ForLifecycleEvent $LifecycleLog "shutdown" $Process.Id
        Assert-ProcessRunning $Process "suspended launch detach"
    } finally {
        Stop-OwnedProcess $Process
    }

    Write-Host "Dark Ages 7.41 loader checks passed"
} finally {
    if ($null -eq $PreviousClientBypass) {
        Remove-Item Env:DARPC_LOADER_TEST_ALLOW_UNSUPPORTED_CLIENT -ErrorAction SilentlyContinue
    } else {
        $env:DARPC_LOADER_TEST_ALLOW_UNSUPPORTED_CLIENT = $PreviousClientBypass
    }
}
