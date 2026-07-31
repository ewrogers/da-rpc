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
$DefaultPort = 2626
$OverridePort = 3626

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
    param(
        [int[]] $ProcessIds,
        [Nullable[int]] $Port = $null
    )

    $StartInfo = [System.Diagnostics.ProcessStartInfo]::new()
    $StartInfo.FileName = $Daemon
    $Arguments = @($ProcessIds | ForEach-Object { "--pid $_" })
    if ($null -ne $Port) {
        $Arguments += @("--port", "$Port")
    }
    $StartInfo.Arguments = ($Arguments -join " ")
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

function Get-ApiJson {
    param(
        [string] $Path,
        [int] $Port
    )

    return Invoke-RestMethod `
        -Uri "http://127.0.0.1:$Port$Path" `
        -Method Get `
        -TimeoutSec 2
}

function Wait-ForApi {
    param(
        [System.Diagnostics.Process] $DaemonProcess,
        [int] $Port,
        [int] $TimeoutMilliseconds = 8000
    )

    $Deadline = [DateTime]::UtcNow.AddMilliseconds($TimeoutMilliseconds)
    do {
        if ($DaemonProcess.HasExited) {
            throw "darpcd.exe exited before its HTTP API became available"
        }
        try {
            $Health = Get-ApiJson -Path "/health" -Port $Port
            if ($Health.status -eq "ok") {
                return
            }
        } catch {
            # The listener may not have entered its accept loop yet.
        }
        Start-Sleep -Milliseconds 50
    } while ([DateTime]::UtcNow -lt $Deadline)

    throw "timed out waiting for the HTTP API on port $Port"
}

function Wait-ForConnectedClients {
    param(
        [int[]] $ProcessIds,
        [int] $Port,
        [int] $TimeoutMilliseconds = 8000
    )

    $Deadline = [DateTime]::UtcNow.AddMilliseconds($TimeoutMilliseconds)
    do {
        try {
            $Response = Get-ApiJson -Path "/clients" -Port $Port
            $Clients = @($Response.clients)
            $AllConnected = $Clients.Count -eq $ProcessIds.Count
            foreach ($ProcessId in $ProcessIds) {
                $Client = @($Clients | Where-Object { $_.pid -eq $ProcessId })
                if ($Client.Count -ne 1 -or $Client[0].status -ne "connected") {
                    $AllConnected = $false
                }
            }
            if ($AllConnected) {
                return $Response
            }
        } catch {
            # Retry while workers complete their handshakes.
        }
        Start-Sleep -Milliseconds 50
    } while ([DateTime]::UtcNow -lt $Deadline)

    throw "timed out waiting for connected clients through the HTTP API"
}

function Assert-ApiContract {
    param(
        [object] $ClientsResponse,
        [int[]] $ProcessIds,
        [int] $Port
    )

    $Clients = @($ClientsResponse.clients)
    Assert-True ($Clients.Count -eq $ProcessIds.Count) "HTTP client count was incorrect"
    foreach ($ProcessId in $ProcessIds) {
        $Client = @($Clients | Where-Object { $_.pid -eq $ProcessId })[0]
        Assert-True ($Client.status -eq "connected") "PID $ProcessId was not connected"
        Assert-True `
            ($Client.identity.instance_id -match "^[0-9A-F]{32}$") `
            "PID $ProcessId had an invalid instance_id"
        Assert-True `
            ($Client.identity.created_time -match "^[0-9]+$") `
            "PID $ProcessId had an invalid created_time"
        Assert-True `
            ($Client.connection.protocol_version -eq "1.0") `
            "PID $ProcessId had the wrong protocol version"
    }

    $OpenApi = Get-ApiJson -Path "/openapi.json" -Port $Port
    Assert-True ($OpenApi.openapi -eq "3.1.0") "OpenAPI version was not 3.1.0"
    $Paths = @($OpenApi.paths.PSObject.Properties.Name)
    Assert-True ($Paths -contains "/health") "OpenAPI omitted /health"
    Assert-True ($Paths -contains "/clients") "OpenAPI omitted /clients"

    $Docs = Invoke-WebRequest `
        -Uri "http://127.0.0.1:$Port/docs/" `
        -UseBasicParsing `
        -TimeoutSec 2
    Assert-True ($Docs.StatusCode -eq 200) "Swagger UI was unavailable"
    $Asset = Invoke-WebRequest `
        -Uri "http://127.0.0.1:$Port/docs/swagger-ui-bundle.js" `
        -UseBasicParsing `
        -TimeoutSec 5
    Assert-True ($Asset.StatusCode -eq 200) "vendored Swagger UI asset was unavailable"
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
    Wait-ForApi $DaemonProcess $DefaultPort
    $PendingClients = @(Get-ApiJson -Path "/clients" -Port $DefaultPort).clients
    Assert-True ($PendingClients.Count -eq 2) "HTTP API omitted configured targets"

    $Result = Invoke-Loader -CommandArgs @("attach", "$($First.Id)", $DarpcDll)
    Assert-True $Result.darpc_loaded "first target attach did not load darpc.dll"
    $Result = Invoke-Loader -CommandArgs @("attach", "$($Second.Id)", $DarpcDll)
    Assert-True $Result.darpc_loaded "second target attach did not load darpc.dll"
    Wait-ForDaemonOwnership $DaemonProcess @($First.Id, $Second.Id)
    $Clients = Wait-ForConnectedClients @($First.Id, $Second.Id) $DefaultPort
    Assert-ApiContract $Clients @($First.Id, $Second.Id) $DefaultPort

    $InitialOutput = Stop-Daemon $DaemonProcess
    $DaemonProcess = $null
    $FirstInitialInstances = Connected-Instances $InitialOutput $First.Id
    $SecondInitialInstances = Connected-Instances $InitialOutput $Second.Id
    Assert-True ($FirstInitialInstances.Count -eq 1) "first target was not registered once"
    Assert-True ($SecondInitialInstances.Count -eq 1) "second target was not registered once"
    Wait-ForDirectConnection $First.Id
    Wait-ForDirectConnection $Second.Id

    Write-Host "Testing daemon restart and independent client replacement"
    $DaemonProcess = Start-Daemon -ProcessIds @($First.Id, $Second.Id) -Port $OverridePort
    Wait-ForApi $DaemonProcess $OverridePort
    Wait-ForDaemonOwnership $DaemonProcess @($First.Id, $Second.Id)
    $Clients = Wait-ForConnectedClients @($First.Id, $Second.Id) $OverridePort
    Assert-ApiContract $Clients @($First.Id, $Second.Id) $OverridePort

    $Result = Invoke-Loader -CommandArgs @("detach", "$($First.Id)", $DarpcDll)
    Assert-True (-not $Result.darpc_loaded) "first target detach left darpc.dll loaded"
    Assert-True (-not $DaemonProcess.HasExited) "one disconnect terminated the daemon"
    Assert-True `
        ((Invoke-DarpcExitCode -ProcessId $Second.Id) -eq 5) `
        "second target stopped being daemon-owned after first disconnect"

    $Result = Invoke-Loader -CommandArgs @("attach", "$($First.Id)", $DarpcDll)
    Assert-True $Result.darpc_loaded "first target reattach did not load darpc.dll"
    Wait-ForDaemonOwnership $DaemonProcess @($First.Id, $Second.Id)
    $Clients = Wait-ForConnectedClients @($First.Id, $Second.Id) $OverridePort
    Assert-ApiContract $Clients @($First.Id, $Second.Id) $OverridePort

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

    $HeldListener = [System.Net.Sockets.TcpListener]::new(
        [System.Net.IPAddress]::Loopback,
        0
    )
    try {
        $HeldListener.Start()
        $OccupiedPort = $HeldListener.LocalEndpoint.Port
        $PreviousErrorActionPreference = $ErrorActionPreference
        try {
            $ErrorActionPreference = "Continue"
            $ConflictOutput = @(& $Daemon --pid $First.Id --port $OccupiedPort 2>&1)
            $ConflictExitCode = $LASTEXITCODE
        } finally {
            $ErrorActionPreference = $PreviousErrorActionPreference
        }
        Assert-True ($ConflictExitCode -eq 1) "occupied port did not fail daemon startup"
        Assert-True `
            (($ConflictOutput -join "`n") -match "failed to listen") `
            "occupied port failure was not explained"
    } finally {
        $HeldListener.Stop()
    }

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
