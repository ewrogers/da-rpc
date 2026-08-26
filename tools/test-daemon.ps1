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
$ManagedPort = 4626
$AutoLoadPort = 5626

foreach ($Path in @($Loader, $Target, $DarpcDll, $Darpc, $Daemon)) {
    if (-not (Test-Path -PathType Leaf $Path)) {
        throw "Required test artifact is missing: $Path"
    }
}

$Loader = (Resolve-Path -LiteralPath $Loader).Path
$Target = (Resolve-Path -LiteralPath $Target).Path
$DarpcDll = (Resolve-Path -LiteralPath $DarpcDll).Path
$Darpc = (Resolve-Path -LiteralPath $Darpc).Path
$Daemon = (Resolve-Path -LiteralPath $Daemon).Path

$LiveClients = @(Get-Process -Name "Darkages" -ErrorAction SilentlyContinue)
if ($LiveClients.Count -gt 0) {
    $LiveProcessIds = ($LiveClients.Id | Sort-Object) -join ", "
    throw "Refusing to run daemon integration tests while Darkages.exe is active (PIDs: $LiveProcessIds)"
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

function ConvertTo-ProcessArguments {
    param([string[]] $Arguments)

    $Quoted = foreach ($Argument in $Arguments) {
        if ($Argument.Contains('"')) {
            throw "Test process arguments cannot contain quotes: $Argument"
        }
        '"' + $Argument + '"'
    }
    return $Quoted -join " "
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
        & $Darpc --output json hello --pid $ProcessId *> $null
        return $LASTEXITCODE
    } finally {
        $ErrorActionPreference = $PreviousErrorActionPreference
    }
}

function Start-Daemon {
    param(
        [int[]] $ProcessIds = @(),
        [Nullable[int]] $Port = $null,
        [switch] $Managed,
        [switch] $AutoLoad
    )

    $StartInfo = [System.Diagnostics.ProcessStartInfo]::new()
    $StartInfo.FileName = $Daemon
    $Arguments = [System.Collections.Generic.List[string]]::new()
    foreach ($ProcessId in $ProcessIds) {
        [void] $Arguments.Add("--pid")
        [void] $Arguments.Add("$ProcessId")
    }
    if ($null -ne $Port) {
        [void] $Arguments.Add("--port")
        [void] $Arguments.Add("$Port")
    }
    if ($Managed) {
        foreach ($Argument in @(
            "--loader-path", $Loader,
            "--dll-path", $DarpcDll
        )) {
            [void] $Arguments.Add($Argument)
        }
    }
    if ($AutoLoad) {
        [void] $Arguments.Add("--auto-load")
    }
    $StartInfo.Arguments = ConvertTo-ProcessArguments $Arguments
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

function Invoke-ApiPost {
    param(
        [string] $Path,
        [int] $Port,
        [AllowNull()]
        [string] $Body = $null
    )

    $Parameters = @{
        Uri = "http://127.0.0.1:$Port$Path"
        Method = "Post"
        TimeoutSec = 15
    }
    if ($null -ne $Body) {
        $Parameters.ContentType = "application/json"
        $Parameters.Body = $Body
    }
    return Invoke-RestMethod @Parameters
}

function Wait-ForClientStatus {
    param(
        [int] $ProcessId,
        [string] $Status,
        [int] $Port,
        [int] $TimeoutMilliseconds = 12000
    )

    $Deadline = [DateTime]::UtcNow.AddMilliseconds($TimeoutMilliseconds)
    $LastObservation = "client absent"
    do {
        try {
            $Clients = @(Get-ApiJson -Path "/clients" -Port $Port).clients
            $Client = @($Clients | Where-Object { $_.pid -eq $ProcessId })
            if ($Client.Count -eq 1 -and $Client[0].status -eq $Status) {
                return $Client[0]
            }
            if ($Client.Count -eq 1) {
                $LastObservation = "status=$($Client[0].status) reason=$($Client[0].reason)"
            } elseif ($Client.Count -gt 1) {
                $LastObservation = "$($Client.Count) matching clients"
            } else {
                $LastObservation = "client absent"
            }
        } catch {
            $LastObservation = "API error: $($_.Exception.Message)"
        }
        Start-Sleep -Milliseconds 50
    } while ([DateTime]::UtcNow -lt $Deadline)

    throw "timed out waiting for PID $ProcessId to reach ${Status}: $LastObservation"
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
            $AllConnected = $true
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
        [object] $ClientList,
        [int[]] $ProcessIds,
        [int] $Port
    )

    $Clients = @($ClientList.clients)
    Assert-True ($Clients.Count -ge $ProcessIds.Count) "HTTP client count was incorrect"
    foreach ($ProcessId in $ProcessIds) {
        $Client = @($Clients | Where-Object { $_.pid -eq $ProcessId })[0]
        Assert-True ($Client.status -eq "connected") "PID $ProcessId was not connected"
        Assert-True `
            ($Client.identity.instance_id -cmatch "^[0-9a-f]{32}$") `
            "PID $ProcessId had an invalid instance_id"
        Assert-True `
            ($Client.identity.created_time -match "^[0-9]+$") `
            "PID $ProcessId had an invalid created_time"
        Assert-True `
            ($Client.connection.protocol_version -eq "1.8") `
            "PID $ProcessId had the wrong protocol version"
        Assert-True `
            ($Client.connection.client_version -eq "7.41") `
            "PID $ProcessId had the wrong client version"
        Assert-True `
            ($null -eq $Client.connection.PSObject.Properties["layout_id"]) `
            "PID $ProcessId exposed obsolete layout_id"
    }

    $OpenApi = Get-ApiJson -Path "/openapi.json" -Port $Port
    Assert-True ($OpenApi.openapi -eq "3.1.0") "OpenAPI version was not 3.1.0"
    $Paths = @($OpenApi.paths.PSObject.Properties.Name)
    Assert-True ($Paths -contains "/health") "OpenAPI omitted /health"
    Assert-True ($Paths -contains "/clients") "OpenAPI omitted /clients"
    Assert-True ($Paths -contains "/clients/launch") "OpenAPI omitted /clients/launch"
    Assert-True ($Paths -contains "/clients/{client}/load") "OpenAPI omitted client load"
    Assert-True ($Paths -contains "/clients/{client}/unload") "OpenAPI omitted client unload"
    $Schemas = @($OpenApi.components.schemas.PSObject.Properties.Name)
    foreach ($Schema in @(
        "ClientIdentity",
        "ClientList",
        "ClientState",
        "ClientStatus",
        "ConnectionMetadata",
        "ErrorDetail",
        "ErrorState",
        "HealthState",
        "HealthStatus",
        "LaunchOptions",
        "LoadResult",
        "LifecycleAction",
        "LifecycleResult",
        "UnloadResult"
    )) {
        Assert-True ($Schemas -contains $Schema) "OpenAPI omitted $Schema"
    }
    Assert-True `
        (@($OpenApi.components.schemas.LaunchOptions.required) -contains "client_path") `
        "OpenAPI did not require launch client_path"
    $ConnectionProperties = @(
        $OpenApi.components.schemas.ConnectionMetadata.properties.PSObject.Properties.Name
    )
    Assert-True `
        ($ConnectionProperties -contains "client_version") `
        "OpenAPI omitted client_version"
    Assert-True `
        ($ConnectionProperties -notcontains "layout_id") `
        "OpenAPI exposed obsolete layout_id"

    $Docs = Invoke-WebRequest `
        -Uri "http://127.0.0.1:$Port/docs/" `
        -UseBasicParsing `
        -TimeoutSec 2
    Assert-True ($Docs.StatusCode -eq 200) "Swagger UI was unavailable"
    $Asset = Invoke-WebRequest `
        -Uri "http://127.0.0.1:$Port/docs/assets/swagger-ui-bundle.js" `
        -UseBasicParsing `
        -TimeoutSec 5
    Assert-True ($Asset.StatusCode -eq 200) "vendored Swagger UI asset was unavailable"

    $Theme = Invoke-WebRequest `
        -Uri "http://127.0.0.1:$Port/docs/ayu.css" `
        -UseBasicParsing `
        -TimeoutSec 2
    Assert-True ($Theme.StatusCode -eq 200) "Swagger UI theme was unavailable"
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
            $Output = $DaemonProcess.StandardOutput.ReadToEnd()
            $ErrorOutput = $DaemonProcess.StandardError.ReadToEnd()
            throw "darpcd.exe exited before owning its target pipes (exit $($DaemonProcess.ExitCode)): $Output $ErrorOutput"
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

    $Pattern = "client pid=$ProcessId status=connected [^`r`n]* instance=([0-9a-f]{32})"
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
    Assert-True ($PendingClients.Count -ge 2) "HTTP API omitted configured targets"

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

Write-Host "Testing automatic managed loading"
$PreviousDiscoveryWindow = $env:DARPC_DISCOVERY_TEST_WINDOW
$MissingProcessId = 2147483646
$ExistingTarget = $null
$FutureTarget = $null
$DaemonProcess = $null
try {
    $env:DARPC_DISCOVERY_TEST_WINDOW = "1"
    Assert-True `
        ($null -eq (Get-Process -Id $MissingProcessId -ErrorAction SilentlyContinue)) `
        "reserved missing-process PID unexpectedly exists"

    $ExistingTarget = Start-Process `
        $Target `
        -ArgumentList "--wait-ms", "60000" `
        -PassThru
    Start-Sleep -Milliseconds 200
    Assert-True (-not $ExistingTarget.HasExited) "existing auto-load target exited during startup"

    $DaemonProcess = Start-Daemon `
        -ProcessIds @($MissingProcessId) `
        -Port $AutoLoadPort `
        -Managed `
        -AutoLoad
    Wait-ForApi $DaemonProcess $AutoLoadPort
    Wait-ForClientStatus $ExistingTarget.Id "connected" $AutoLoadPort | Out-Null
    Wait-ForClientStatus $MissingProcessId "not_loaded" $AutoLoadPort | Out-Null

    $FutureTarget = Start-Process `
        $Target `
        -ArgumentList "--wait-ms", "60000" `
        -PassThru
    Wait-ForClientStatus $FutureTarget.Id "connected" $AutoLoadPort | Out-Null

    $Result = Invoke-ApiPost `
        -Path "/clients/$($ExistingTarget.Id)/unload" `
        -Port $AutoLoadPort
    Assert-True $Result.was_unloaded "explicit unload did not remove the automatically loaded DLL"
    Wait-ForClientStatus $ExistingTarget.Id "not_loaded" $AutoLoadPort | Out-Null
    Start-Sleep -Milliseconds 1500
    $Client = Wait-ForClientStatus $ExistingTarget.Id "not_loaded" $AutoLoadPort
    Assert-True `
        ($Client.status -eq "not_loaded") `
        "auto-load reversed an explicit unload"

    $DaemonProcess.Kill()
    $DaemonProcess.WaitForExit()
    $AutoLoadOutput = $DaemonProcess.StandardOutput.ReadToEnd()
    $AutoLoadErrors = $DaemonProcess.StandardError.ReadToEnd()
    $DaemonProcess.Dispose()
    $DaemonProcess = $null
    foreach ($Process in @($ExistingTarget, $FutureTarget)) {
        Assert-True `
            ($AutoLoadOutput -match "client pid=$($Process.Id) auto-load=loaded") `
            "PID $($Process.Id) did not report one automatic load"
    }
    $FailurePattern = "client pid=$MissingProcessId auto-load failed"
    Assert-True `
        ([regex]::Matches($AutoLoadErrors, $FailurePattern).Count -eq 1) `
        "the missing candidate did not fail automatic loading exactly once"
} finally {
    if ($null -ne $DaemonProcess) {
        if (-not $DaemonProcess.HasExited) {
            $DaemonProcess.Kill()
            $DaemonProcess.WaitForExit()
        }
        $FailureOutput = $DaemonProcess.StandardOutput.ReadToEnd()
        $FailureErrors = $DaemonProcess.StandardError.ReadToEnd()
        if (-not [string]::IsNullOrWhiteSpace($FailureOutput)) {
            Write-Warning "auto-load daemon stdout before cleanup:`n$FailureOutput"
        }
        if (-not [string]::IsNullOrWhiteSpace($FailureErrors)) {
            Write-Warning "auto-load daemon stderr before cleanup:`n$FailureErrors"
        }
        $DaemonProcess.Dispose()
    }
    foreach ($Process in @($ExistingTarget, $FutureTarget)) {
        if ($null -ne $Process -and -not $Process.HasExited) {
            try {
                Invoke-Loader -CommandArgs @("detach", "$($Process.Id)", $DarpcDll) | Out-Null
            } catch {
                # The target is test-owned and is stopped below even when cleanup fails.
            }
            Stop-Target $Process
        }
    }
    if ($null -eq $PreviousDiscoveryWindow) {
        Remove-Item Env:DARPC_DISCOVERY_TEST_WINDOW -ErrorAction SilentlyContinue
    } else {
        $env:DARPC_DISCOVERY_TEST_WINDOW = $PreviousDiscoveryWindow
    }
}

Write-Host "Testing discovery and managed lifecycle API"
$PreviousDiscoveryWindow = $env:DARPC_DISCOVERY_TEST_WINDOW
$DiscoveredTarget = $null
$LaunchedTarget = $null
$DaemonProcess = $null
try {
    $env:DARPC_DISCOVERY_TEST_WINDOW = "1"
    $DiscoveredTarget = Start-Process `
        $Target `
        -ArgumentList "--wait-ms", "60000" `
        -PassThru
    Start-Sleep -Milliseconds 200
    Assert-True (-not $DiscoveredTarget.HasExited) "discovery target exited during startup"

    $DaemonProcess = Start-Daemon -Port $ManagedPort -Managed
    Wait-ForApi $DaemonProcess $ManagedPort
    Wait-ForClientStatus $DiscoveredTarget.Id "not_loaded" $ManagedPort | Out-Null

    $Result = Invoke-ApiPost `
        -Path "/clients/$($DiscoveredTarget.Id)/load" `
        -Port $ManagedPort
    Assert-True ($Result.operation -eq "load") "managed load reported the wrong operation"
    Assert-True $Result.was_loaded "managed load did not report a DLL transition"
    Assert-True (-not ($Result.PSObject.Properties.Name -contains "changed")) `
        "managed load exposed the redundant changed field"
    Wait-ForClientStatus $DiscoveredTarget.Id "connected" $ManagedPort | Out-Null

    $Result = Invoke-ApiPost `
        -Path "/clients/$($DiscoveredTarget.Id)/unload" `
        -Port $ManagedPort
    Assert-True ($Result.operation -eq "unload") "managed unload reported the wrong operation"
    Assert-True $Result.was_unloaded "managed unload did not report a DLL transition"
    Assert-True (-not ($Result.PSObject.Properties.Name -contains "changed")) `
        "managed unload exposed the redundant changed field"
    Wait-ForClientStatus $DiscoveredTarget.Id "not_loaded" $ManagedPort | Out-Null

    Invoke-ApiPost `
        -Path "/clients/$($DiscoveredTarget.Id)/load" `
        -Port $ManagedPort | Out-Null
    Wait-ForClientStatus $DiscoveredTarget.Id "connected" $ManagedPort | Out-Null

    $LaunchBody = @{ client_path = $Target } | ConvertTo-Json -Compress
    $Result = Invoke-ApiPost `
        -Path "/clients/launch" `
        -Port $ManagedPort `
        -Body $LaunchBody
    Assert-True ($Result.operation -eq "launch") "managed launch reported the wrong operation"
    Assert-True $Result.darpc_loaded "managed launch did not initialize darpc.dll"
    $LaunchedTarget = Get-Process -Id $Result.pid -ErrorAction Stop
    Wait-ForClientStatus $LaunchedTarget.Id "connected" $ManagedPort | Out-Null

    $RejectedStatus = $null
    try {
        Invoke-ApiPost `
            -Path "/clients/launch" `
            -Port $ManagedPort `
            -Body (@{
                client_path = $Target
                arguments = @("not-supported")
            } | ConvertTo-Json -Compress) | Out-Null
    } catch {
        $RejectedStatus = [int] $_.Exception.Response.StatusCode
    }
    Assert-True `
        ($RejectedStatus -eq 422) `
        "managed launch accepted arbitrary client arguments"

    $Clients = Wait-ForConnectedClients `
        @($DiscoveredTarget.Id, $LaunchedTarget.Id) `
        $ManagedPort
    Assert-ApiContract `
        $Clients `
        @($DiscoveredTarget.Id, $LaunchedTarget.Id) `
        $ManagedPort

    Stop-Daemon $DaemonProcess | Out-Null
    $DaemonProcess = $null
    Wait-ForDirectConnection $DiscoveredTarget.Id
    Wait-ForDirectConnection $LaunchedTarget.Id
} finally {
    if ($null -ne $DaemonProcess) {
        if (-not $DaemonProcess.HasExited) {
            $DaemonProcess.Kill()
            $DaemonProcess.WaitForExit()
        }
        $DaemonProcess.Dispose()
    }
    foreach ($Process in @($DiscoveredTarget, $LaunchedTarget)) {
        if ($null -ne $Process -and -not $Process.HasExited) {
            try {
                Invoke-Loader -CommandArgs @("detach", "$($Process.Id)", $DarpcDll) | Out-Null
            } catch {
                # The target is test-owned and is stopped below even when cleanup fails.
            }
            Stop-Target $Process
        }
    }
    if ($null -eq $PreviousDiscoveryWindow) {
        Remove-Item Env:DARPC_DISCOVERY_TEST_WINDOW -ErrorAction SilentlyContinue
    } else {
        $env:DARPC_DISCOVERY_TEST_WINDOW = $PreviousDiscoveryWindow
    }
}

Write-Host "Daemon discovery and management integration checks passed"
exit 0
