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

foreach ($Path in @($Loader, $Target, $DarpcDll, $Darpc)) {
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

function Invoke-JsonCommand {
    param(
        [string] $Executable,
        [string[]] $CommandArgs,
        [int] $ExpectedExitCode = 0
    )

    $PreviousErrorActionPreference = $ErrorActionPreference

    try {
        $ErrorActionPreference = "Continue"
        $Output = @(& $Executable @CommandArgs 2>$null)
        $ExitCode = $LASTEXITCODE
    } finally {
        $ErrorActionPreference = $PreviousErrorActionPreference
    }

    if ($ExitCode -ne $ExpectedExitCode) {
        throw "$Executable exited with $ExitCode, expected $ExpectedExitCode for: $CommandArgs"
    }
    if ($Output.Count -ne 1) {
        throw "$Executable emitted $($Output.Count) stdout lines, expected one JSON result"
    }

    return $Output[0] | ConvertFrom-Json
}

function Invoke-Loader {
    param(
        [string[]] $CommandArgs,
        [int] $ExpectedExitCode = 0
    )

    return Invoke-JsonCommand `
        -Executable $Loader `
        -CommandArgs (@("--json") + $CommandArgs) `
        -ExpectedExitCode $ExpectedExitCode
}

function Invoke-Darpc {
    param(
        [string[]] $CommandArgs,
        [int] $ExpectedExitCode = 0
    )

    return Invoke-JsonCommand `
        -Executable $Darpc `
        -CommandArgs (@("--output", "json") + $CommandArgs) `
        -ExpectedExitCode $ExpectedExitCode
}

function Wait-ForHello {
    param(
        [int] $ProcessId,
        [int] $TimeoutMilliseconds = 5000
    )

    $Deadline = [DateTime]::UtcNow.AddMilliseconds($TimeoutMilliseconds)
    do {
        $PreviousErrorActionPreference = $ErrorActionPreference
        try {
            $ErrorActionPreference = "Continue"
            $Output = @(& $Darpc --output json hello --pid $ProcessId 2>$null)
            $ExitCode = $LASTEXITCODE
        } finally {
            $ErrorActionPreference = $PreviousErrorActionPreference
        }

        if ($ExitCode -eq 0 -and $Output.Count -eq 1) {
            return $Output[0] | ConvertFrom-Json
        }
        Start-Sleep -Milliseconds 25
    } while ([DateTime]::UtcNow -lt $Deadline)

    throw "timed out waiting for the IPC endpoint for process $ProcessId"
}

function Connect-RawPipe {
    param([int] $ProcessId)

    $Pipe = [System.IO.Pipes.NamedPipeClientStream]::new(
        ".",
        "da-rpc-$ProcessId",
        [System.IO.Pipes.PipeDirection]::InOut,
        [System.IO.Pipes.PipeOptions]::Asynchronous
    )
    $Pipe.Connect(5000)
    return $Pipe
}

function Assert-TargetRunning {
    param(
        [System.Diagnostics.Process] $Process,
        [string] $Context
    )

    Assert-True (-not $Process.HasExited) "target exited after $Context"
}

$Process = Start-Process `
    -FilePath $Target `
    -ArgumentList "--wait-ms", "60000" `
    -PassThru
$RawPipe = $null

try {
    Start-Sleep -Milliseconds 200
    Assert-TargetRunning $Process "startup"
    $LogPath = Join-Path $env:USERPROFILE "darpc\logs\pid-$($Process.Id).log"
    Remove-Item -LiteralPath $LogPath -Force -ErrorAction SilentlyContinue

    Write-Host "Testing direct hello, ping, tick health, and byte-exact echo"
    $Result = Invoke-Loader -CommandArgs @("attach", "$($Process.Id)", $DarpcDll)
    Assert-True $Result.darpc_loaded "attach did not observe darpc.dll"

    $Hello = Invoke-Darpc -CommandArgs @("hello", "--pid", "$($Process.Id)")
    Assert-True ($Hello.command -eq "hello") "hello command identity was incorrect"
    Assert-True ($Hello.pid -eq $Process.Id) "hello reported the wrong PID"
    Assert-True ($Hello.protocol_version -eq "1.1") "hello negotiated an unexpected protocol"
    Assert-True ($Hello.architecture -eq "x86") "hello reported an unexpected architecture"
    Assert-True ($Hello.sequence -eq 0) "hello sequence was not zero"

    $Ping = Invoke-Darpc -CommandArgs @("ping", "--pid", "$($Process.Id)")
    Assert-True ($Ping.request_id -eq 1) "ping request ID was not one"
    Assert-True ($Ping.request_sequence -eq 1) "ping request sequence was not one"
    Assert-True ($Ping.response_sequence -eq 1) "ping response sequence was not one"

    $TickHealth = Invoke-Darpc -CommandArgs @("tick", "health", "--pid", "$($Process.Id)")
    Assert-True ($TickHealth.command -eq "tick health") "tick health command identity was incorrect"
    Assert-True (-not $TickHealth.installed) "controlled target unexpectedly installed the game tick hook"
    Assert-True (-not $TickHealth.advancing) "controlled target unexpectedly reported advancing ticks"
    Assert-True ($TickHealth.relocated_bytes -eq 0) "controlled target reported relocated tick bytes"
    Assert-True ($TickHealth.tick_count -eq 0) "controlled target reported game ticks"

    $Log = Get-Content -Raw -LiteralPath $LogPath
    Assert-True ($Log -match "event=hook_skipped") "DLL log did not record the controlled hook skip"
    Assert-True ($Log -match "event=hook_health") "DLL log did not record worker-side hook health"

    $EchoText = "M6 byte-exact echo payload 0123"
    $Echo = Invoke-Darpc -CommandArgs @("echo", "--pid", "$($Process.Id)", $EchoText)
    Assert-True ($Echo.request_id -eq 1) "echo request ID was not one"
    Assert-True `
        ($Echo.text -ceq $EchoText) `
        "echo text differed: expected '$EchoText', received '$($Echo.text)'"
    $ExpectedBytes = [System.Text.Encoding]::UTF8.GetByteCount($EchoText)
    Assert-True ($Echo.bytes -eq $ExpectedBytes) "echo byte count was incorrect"

    Write-Host "Testing missing and busy endpoint errors"
    $Missing = Invoke-Darpc `
        -CommandArgs @("hello", "--pid", "$PID") `
        -ExpectedExitCode 4
    Assert-True ($Missing.error.kind -eq "pipe_missing") "missing endpoint error was not distinct"

    $RawPipe = Connect-RawPipe -ProcessId $Process.Id
    $Busy = Invoke-Darpc `
        -CommandArgs @("hello", "--pid", "$($Process.Id)") `
        -ExpectedExitCode 5
    Assert-True ($Busy.error.kind -eq "pipe_busy") "busy endpoint error was not distinct"
    Assert-True ($Busy.error.message -match "darpcd") "busy error did not explain daemon ownership"

    Write-Host "Testing malformed-client isolation and reconnect"
    $MalformedHeader = [byte[]]::new(20)
    $RawPipe.Write($MalformedHeader, 0, $MalformedHeader.Length)
    $RawPipe.Flush()
    $RawPipe.Dispose()
    $RawPipe = $null
    $Hello = Wait-ForHello -ProcessId $Process.Id
    Assert-True ($Hello.pid -eq $Process.Id) "server did not recover after malformed input"
    Assert-TargetRunning $Process "malformed input"

    Write-Host "Testing bounded shutdown with pending client I/O"
    $RawPipe = Connect-RawPipe -ProcessId $Process.Id
    $Stopwatch = [System.Diagnostics.Stopwatch]::StartNew()
    $Result = Invoke-Loader -CommandArgs @("detach", "$($Process.Id)", $DarpcDll)
    $Stopwatch.Stop()
    Assert-True (-not $Result.darpc_loaded) "detach left darpc.dll loaded"
    Assert-True ($Stopwatch.ElapsedMilliseconds -lt 6000) "pending I/O shutdown exceeded its bound"
    $RawPipe.Dispose()
    $RawPipe = $null
    Assert-TargetRunning $Process "pending I/O shutdown"

    Write-Host "Testing bounded shutdown while waiting for a connection"
    $Result = Invoke-Loader -CommandArgs @("attach", "$($Process.Id)", $DarpcDll)
    Assert-True $Result.darpc_loaded "reattach did not observe darpc.dll"
    $Stopwatch.Restart()
    $Result = Invoke-Loader -CommandArgs @("detach", "$($Process.Id)", $DarpcDll)
    $Stopwatch.Stop()
    Assert-True (-not $Result.darpc_loaded) "final detach left darpc.dll loaded"
    Assert-True ($Stopwatch.ElapsedMilliseconds -lt 6000) "accept cancellation exceeded its bound"
    Assert-TargetRunning $Process "accept cancellation"
} finally {
    if ($null -ne $RawPipe) {
        $RawPipe.Dispose()
    }
    if ($null -ne $Process -and -not $Process.HasExited) {
        Stop-Process -Id $Process.Id -Force
        $Process.WaitForExit()
    }
}

Write-Host "IPC integration checks passed"
