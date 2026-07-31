param(
    [Parameter(Mandatory = $true)]
    [string] $X86TargetDir,

    [Parameter(Mandatory = $true)]
    [string] $X64TargetDir
)

$ErrorActionPreference = "Stop"
$env:DARPC_LOADER_TEST_ALLOW_UNSUPPORTED_CLIENT = "1"

$Loader = Join-Path $X86TargetDir "loader.exe"
$Target = Join-Path $X86TargetDir "injection-target.exe"
$DarpcDll = Join-Path $X86TargetDir "darpc.dll"
$Darpc = Join-Path $X64TargetDir "darpc.exe"
$Daemon = Join-Path $X64TargetDir "darpcd.exe"

foreach ($Path in @($Loader, $Target, $DarpcDll, $Darpc, $Daemon)) {
    if (-not (Test-Path -PathType Leaf $Path)) {
        throw "Required test artifact is missing: $Path"
    }
}

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
    param([string[]] $CommandArgs)

    $PreviousErrorActionPreference = $ErrorActionPreference
    try {
        $ErrorActionPreference = "Continue"
        $Output = @(& $Loader --json @CommandArgs 2>$null)
        $ExitCode = $LASTEXITCODE
    } finally {
        $ErrorActionPreference = $PreviousErrorActionPreference
    }
    if ($ExitCode -ne 0) {
        throw "loader exited with $ExitCode for: $CommandArgs"
    }
    return $Output[-1] | ConvertFrom-Json
}

function Invoke-DarpcExitCode {
    param([int] $ProcessId)

    $PreviousErrorActionPreference = $ErrorActionPreference
    try {
        $ErrorActionPreference = "Continue"
        & $Darpc --output json ipc hello --pid $ProcessId *> $null
        return $LASTEXITCODE
    } finally {
        $ErrorActionPreference = $PreviousErrorActionPreference
    }
}

function Start-Daemon {
    param([int[]] $ProcessIds)

    $StartInfo = [System.Diagnostics.ProcessStartInfo]::new()
    $StartInfo.FileName = $Daemon
    $StartInfo.Arguments = (($ProcessIds | ForEach-Object { "--pid $_" }) -join " ")
    $StartInfo.UseShellExecute = $false
    $StartInfo.CreateNoWindow = $true
    $StartInfo.RedirectStandardOutput = $true
    $StartInfo.RedirectStandardError = $true

    $Process = [System.Diagnostics.Process]::new()
    $Process.StartInfo = $StartInfo
    if (-not $Process.Start()) {
        $Process.Dispose()
        throw "failed to start darpcd.exe"
    }
    return $Process
}

function Stop-Daemon {
    param([System.Diagnostics.Process] $Process)

    if (-not $Process.HasExited) {
        $Process.Kill()
        $Process.WaitForExit()
    }
    $Output = $Process.StandardOutput.ReadToEnd()
    $ErrorOutput = $Process.StandardError.ReadToEnd()
    $Process.Dispose()
    if ($ErrorOutput) {
        throw "darpcd.exe wrote an error: $ErrorOutput"
    }
    return $Output
}

function Wait-ForDaemonOwnership {
    param(
        [System.Diagnostics.Process] $DaemonProcess,
        [int[]] $ProcessIds,
        [int] $TimeoutMilliseconds = 8000
    )

    $Deadline = [DateTime]::UtcNow.AddMilliseconds($TimeoutMilliseconds)
    do {
        if ($DaemonProcess.HasExited) {
            throw "darpcd.exe exited before owning its target pipes"
        }
        $Owned = $true
        foreach ($ProcessId in $ProcessIds) {
            if ((Invoke-DarpcExitCode -ProcessId $ProcessId) -ne 5) {
                $Owned = $false
            }
        }
        if ($Owned) {
            return
        }
        Start-Sleep -Milliseconds 50
    } while ([DateTime]::UtcNow -lt $Deadline)

    throw "timed out waiting for darpcd.exe to own every target pipe"
}

function Wait-ForDirectConnection {
    param(
        [int] $ProcessId,
        [int] $TimeoutMilliseconds = 8000
    )

    $Deadline = [DateTime]::UtcNow.AddMilliseconds($TimeoutMilliseconds)
    do {
        if ((Invoke-DarpcExitCode -ProcessId $ProcessId) -eq 0) {
            return
        }
        Start-Sleep -Milliseconds 50
    } while ([DateTime]::UtcNow -lt $Deadline)

    throw "timed out waiting to connect directly to PID $ProcessId"
}

function Connected-Instances {
    param(
        [string] $Output,
        [int] $ProcessId
    )

    $Pattern = "client pid=$ProcessId status=connected [^`r`n]* instance=([0-9A-F]{32})"
    return @([regex]::Matches($Output, $Pattern) | ForEach-Object { $_.Groups[1].Value })
}

function Stop-Target {
    param([System.Diagnostics.Process] $Process)

    if ($null -ne $Process -and -not $Process.HasExited) {
        Stop-Process -Id $Process.Id -Force
        $Process.WaitForExit()
    }
}

$First = $null
$Second = $null
$DaemonProcess = $null

try {
    $First = Start-Process $Target -ArgumentList "--wait-ms", "60000" -PassThru
    $Second = Start-Process $Target -ArgumentList "--wait-ms", "60000" -PassThru
    Start-Sleep -Milliseconds 200
    Assert-True (-not $First.HasExited) "first injection target exited during startup"
    Assert-True (-not $Second.HasExited) "second injection target exited during startup"

    Write-Host "Testing daemon-first connection to two target PIDs"
    $DaemonProcess = Start-Daemon -ProcessIds @($First.Id, $Second.Id)
    Start-Sleep -Milliseconds 200
    Assert-True (-not $DaemonProcess.HasExited) "daemon exited while target pipes were missing"

    $Result = Invoke-Loader -CommandArgs @("attach", "$($First.Id)", $DarpcDll)
    Assert-True $Result.darpc_loaded "first target attach did not load darpc.dll"
    $Result = Invoke-Loader -CommandArgs @("attach", "$($Second.Id)", $DarpcDll)
    Assert-True $Result.darpc_loaded "second target attach did not load darpc.dll"
    Wait-ForDaemonOwnership $DaemonProcess @($First.Id, $Second.Id)

    $InitialOutput = Stop-Daemon $DaemonProcess
    $DaemonProcess = $null
    $FirstInitialInstances = Connected-Instances $InitialOutput $First.Id
    $SecondInitialInstances = Connected-Instances $InitialOutput $Second.Id
    Assert-True ($FirstInitialInstances.Count -eq 1) "first target was not registered once"
    Assert-True ($SecondInitialInstances.Count -eq 1) "second target was not registered once"
    Wait-ForDirectConnection $First.Id
    Wait-ForDirectConnection $Second.Id

    Write-Host "Testing daemon restart and independent client replacement"
    $DaemonProcess = Start-Daemon -ProcessIds @($First.Id, $Second.Id)
    Wait-ForDaemonOwnership $DaemonProcess @($First.Id, $Second.Id)

    $Result = Invoke-Loader -CommandArgs @("detach", "$($First.Id)", $DarpcDll)
    Assert-True (-not $Result.darpc_loaded) "first target detach left darpc.dll loaded"
    Assert-True (-not $DaemonProcess.HasExited) "one disconnect terminated the daemon"
    Assert-True `
        ((Invoke-DarpcExitCode -ProcessId $Second.Id) -eq 5) `
        "second target stopped being daemon-owned after first disconnect"

    $Result = Invoke-Loader -CommandArgs @("attach", "$($First.Id)", $DarpcDll)
    Assert-True $Result.darpc_loaded "first target reattach did not load darpc.dll"
    Wait-ForDaemonOwnership $DaemonProcess @($First.Id, $Second.Id)

    $RestartOutput = Stop-Daemon $DaemonProcess
    $DaemonProcess = $null
    Assert-True `
        ($RestartOutput -match "client pid=$($First.Id) status=disconnected") `
        "first target disconnect was not visible"
    $FirstRestartInstances = Connected-Instances $RestartOutput $First.Id
    $SecondRestartInstances = Connected-Instances $RestartOutput $Second.Id
    Assert-True ($FirstRestartInstances.Count -eq 2) "first target replacement was not registered"
    Assert-True `
        (($FirstRestartInstances | Select-Object -Unique).Count -eq 2) `
        "first target replacement reused its DLL instance identity"
    Assert-True ($SecondRestartInstances.Count -eq 1) "second target was unexpectedly re-registered"
    Assert-True `
        ($SecondRestartInstances[0] -eq $SecondInitialInstances[0]) `
        "daemon restart changed the second DLL instance identity"

    Wait-ForDirectConnection $First.Id
    Wait-ForDirectConnection $Second.Id
} finally {
    if ($null -ne $DaemonProcess) {
        if (-not $DaemonProcess.HasExited) {
            $DaemonProcess.Kill()
            $DaemonProcess.WaitForExit()
        }
        $DaemonProcess.Dispose()
    }
    foreach ($Process in @($First, $Second)) {
        if ($null -ne $Process -and -not $Process.HasExited) {
            try {
                Invoke-Loader -CommandArgs @("detach", "$($Process.Id)", $DarpcDll) | Out-Null
            } catch {
                # The target is test-owned and is stopped below even when cleanup fails.
            }
            Stop-Target $Process
        }
    }
}

Write-Host "Daemon registry integration checks passed"
