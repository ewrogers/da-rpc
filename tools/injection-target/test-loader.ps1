param(
    [Parameter(Mandatory = $true)]
    [string] $TargetDir
)

$ErrorActionPreference = "Stop"
$env:DARPC_LOADER_TEST_ALLOW_UNSUPPORTED_CLIENT = "1"

$Loader = Join-Path $TargetDir "loader.exe"
$Target = Join-Path $TargetDir "injection-target.exe"
$DarpcDll = Join-Path $TargetDir "darpc.dll"
$FixtureDll = Join-Path $TargetDir "darpc_fixture.dll"
$FixtureDirectory = Join-Path ([System.IO.Path]::GetTempPath()) "darpc-loader-fixture"
$FixtureDarpcDll = Join-Path $FixtureDirectory "darpc.dll"

foreach ($Path in @($Loader, $Target, $DarpcDll, $FixtureDll)) {
    if (-not (Test-Path -PathType Leaf $Path)) {
        throw "Required test artifact is missing: $Path"
    }
}

New-Item -ItemType Directory -Force -Path $FixtureDirectory | Out-Null
Copy-Item -Force $FixtureDll $FixtureDarpcDll

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

function Start-InjectionTarget {
    param(
        [string] $Mode = ""
    )

    $PreviousMode = $env:DARPC_LOADER_TEST_MODE

    try {
        if ($Mode) {
            $env:DARPC_LOADER_TEST_MODE = $Mode
        } else {
            Remove-Item Env:DARPC_LOADER_TEST_MODE -ErrorAction SilentlyContinue
        }

        $Process = Start-Process `
            -FilePath $Target `
            -ArgumentList "--wait-ms", "30000" `
            -PassThru
    } finally {
        if ($null -eq $PreviousMode) {
            Remove-Item Env:DARPC_LOADER_TEST_MODE -ErrorAction SilentlyContinue
        } else {
            $env:DARPC_LOADER_TEST_MODE = $PreviousMode
        }
    }

    Start-Sleep -Milliseconds 200
    Assert-True (-not $Process.HasExited) "injection target exited during startup"
    return $Process
}

function Stop-InjectionTarget {
    param(
        [System.Diagnostics.Process] $Process
    )

    if ($null -ne $Process -and -not $Process.HasExited) {
        Stop-Process -Id $Process.Id -Force
        $Process.WaitForExit()
    }
}

function Assert-TargetRunning {
    param(
        [System.Diagnostics.Process] $Process,
        [string] $Context
    )

    Assert-True (-not $Process.HasExited) "target exited after $Context"
}

function Wait-ForFile {
    param(
        [string] $Path,
        [int] $TimeoutMilliseconds = 5000
    )

    $Deadline = [DateTime]::UtcNow.AddMilliseconds($TimeoutMilliseconds)

    while (-not (Test-Path -PathType Leaf $Path)) {
        if ([DateTime]::UtcNow -ge $Deadline) {
            throw "timed out waiting for file: $Path"
        }

        Start-Sleep -Milliseconds 25
    }
}

function Assert-ProcessExited {
    param(
        [int] $ProcessId,
        [string] $Context,
        [int] $TimeoutMilliseconds = 5000
    )

    $Deadline = [DateTime]::UtcNow.AddMilliseconds($TimeoutMilliseconds)

    while ($null -ne (Get-Process -Id $ProcessId -ErrorAction SilentlyContinue)) {
        if ([DateTime]::UtcNow -ge $Deadline) {
            throw "process $ProcessId remained running after $Context"
        }

        Start-Sleep -Milliseconds 25
    }
}

Write-Host "Testing successful loader lifecycle"
$Process = Start-InjectionTarget
try {
    $Result = Invoke-Loader -CommandArgs @("inspect", "$($Process.Id)")
    Assert-True (-not $Result.darpc_loaded) "initial inspect reported darpc.dll loaded"

    $Result = Invoke-Loader -CommandArgs @("attach", "$($Process.Id)", $DarpcDll)
    Assert-True $Result.changed "attach did not report a state change"
    Assert-True $Result.darpc_loaded "attach did not observe darpc.dll"

    $Result = Invoke-Loader `
        -CommandArgs @("attach", "$($Process.Id)", $DarpcDll) `
        -ExpectedExitCode 9
    Assert-True ($Result.error.kind -eq "already_loaded") "duplicate attach error was not distinct"
    Assert-TargetRunning $Process "duplicate attach"

    $Result = Invoke-Loader -CommandArgs @("inspect", "$($Process.Id)")
    Assert-True $Result.darpc_loaded "inspect did not observe attached darpc.dll"

    $Result = Invoke-Loader -CommandArgs @("detach", "$($Process.Id)", $DarpcDll)
    Assert-True $Result.changed "detach did not report a state change"
    Assert-True (-not $Result.darpc_loaded) "detach left darpc.dll loaded"

    $Result = Invoke-Loader -CommandArgs @("detach", "$($Process.Id)", $DarpcDll)
    Assert-True (-not $Result.changed) "repeated detach reported a state change"
    Assert-True (-not $Result.darpc_loaded) "repeated detach reported darpc.dll loaded"
    Assert-TargetRunning $Process "repeated detach"
} finally {
    Stop-InjectionTarget $Process
}

Write-Host "Testing initialization failure rollback"
$Process = Start-InjectionTarget "init-fail"
try {
    $Result = Invoke-Loader `
        -CommandArgs @("attach", "$($Process.Id)", $FixtureDarpcDll) `
        -ExpectedExitCode 11
    Assert-True ($Result.error.kind -eq "initialization_failed") "initialization failure was not distinct"
    Assert-TargetRunning $Process "initialization failure"

    $Result = Invoke-Loader -CommandArgs @("inspect", "$($Process.Id)")
    Assert-True (-not $Result.darpc_loaded) "initialization rollback left darpc.dll loaded"
} finally {
    Stop-InjectionTarget $Process
}

Write-Host "Testing shutdown failure"
$Process = Start-InjectionTarget "shutdown-fail"
try {
    $Result = Invoke-Loader -CommandArgs @("attach", "$($Process.Id)", $FixtureDarpcDll)
    Assert-True $Result.darpc_loaded "fixture attach did not load darpc.dll"

    $Result = Invoke-Loader `
        -CommandArgs @("detach", "$($Process.Id)", $FixtureDarpcDll) `
        -ExpectedExitCode 12
    Assert-True ($Result.error.kind -eq "shutdown_failed") "shutdown failure was not distinct"
    Assert-TargetRunning $Process "shutdown failure"

    $Result = Invoke-Loader -CommandArgs @("inspect", "$($Process.Id)")
    Assert-True $Result.darpc_loaded "failed shutdown unloaded darpc.dll"
} finally {
    Stop-InjectionTarget $Process
}

Write-Host "Testing remote thread timeout"
$Process = Start-InjectionTarget "init-timeout"
try {
    $Result = Invoke-Loader `
        -CommandArgs @("attach", "$($Process.Id)", $FixtureDarpcDll) `
        -ExpectedExitCode 10
    Assert-True ($Result.error.kind -eq "timeout") "remote timeout was not distinct"
    Assert-TargetRunning $Process "remote timeout"

    $Result = Invoke-Loader -CommandArgs @("inspect", "$($Process.Id)")
    Assert-True $Result.darpc_loaded "timed-out initialization unexpectedly unloaded darpc.dll"
} finally {
    Stop-InjectionTarget $Process
}

Write-Host "Testing process failure classifications"
$Result = Invoke-Loader -CommandArgs @("inspect", "4294967295") -ExpectedExitCode 5
Assert-True ($Result.error.kind -eq "process_missing") "missing process was not distinct"

$Result = Invoke-Loader -CommandArgs @("inspect", "$PID") -ExpectedExitCode 8
Assert-True ($Result.error.kind -eq "wrong_architecture") "wrong architecture was not distinct"

$Result = Invoke-Loader -CommandArgs @("attach", "4", $DarpcDll) -ExpectedExitCode 7
Assert-True ($Result.error.kind -eq "access_denied") "access denied was not distinct"

Write-Host "Testing suspended launch lifecycle"
$LaunchDirectory = Join-Path `
    ([System.IO.Path]::GetTempPath()) `
    "darpc loader launch $([Guid]::NewGuid().ToString('N'))"
$LaunchTarget = Join-Path $LaunchDirectory "Target With Spaces.exe"
$LaunchReport = Join-Path $LaunchDirectory "launch report.txt"
$LaunchExitReport = Join-Path $LaunchDirectory "exit report.txt"
$LaunchFailureReport = Join-Path $LaunchDirectory "failure report.txt"
$Payload = @("plain", "two words", 'trailing\', "snowman-$([char]0x2603)")

New-Item -ItemType Directory -Force -Path $LaunchDirectory | Out-Null
Copy-Item -Force $Target $LaunchTarget

$Process = $null
try {
    $LaunchArguments = @(
        "launch",
        $LaunchTarget,
        $DarpcDll,
        "--",
        "--wait-ms",
        "30000",
        "--launch-report",
        $LaunchReport,
        "--"
    )
    $LaunchArguments += $Payload

    $Result = Invoke-Loader -CommandArgs $LaunchArguments
    Assert-True ($Result.command -eq "launch") "launch result had the wrong command"
    Assert-True $Result.changed "launch did not report a state change"
    Assert-True $Result.darpc_loaded "launch did not observe darpc.dll"

    $Process = Get-Process -Id $Result.pid -ErrorAction Stop
    Wait-ForFile $LaunchReport
    $ReportLines = @(Get-Content -Encoding UTF8 $LaunchReport)
    $ExpectedDirectory = (Resolve-Path $LaunchDirectory).Path
    $CurrentDirectoryLine = @(
        $ReportLines | Where-Object { $_.StartsWith("cwd=") }
    )[0]
    $ReportedDirectory = $CurrentDirectoryLine.Substring(4)

    if ($ReportedDirectory.StartsWith("\\?\")) {
        $ReportedDirectory = $ReportedDirectory.Substring(4)
    }

    Assert-True ($ReportedDirectory -eq $ExpectedDirectory) "launch used the wrong current directory"
    Assert-True ($ReportLines -contains "darpc_loaded_at_start=true") "darpc.dll was absent when the target started"
    Assert-True ($ReportLines -contains "initialized_at_start=true") "darpc.dll initialized after target startup"
    Assert-True `
        ($ReportLines -contains "standard_handles_unavailable_at_start=true") `
        "launched target inherited standard handles"

    $ArgumentLines = @($ReportLines | Where-Object { $_.StartsWith("arg=") })
    Assert-True ($ArgumentLines.Count -eq $Payload.Count) "launch forwarded the wrong argument count"

    for ($Index = 0; $Index -lt $Payload.Count; $Index++) {
        Assert-True `
            ($ArgumentLines[$Index] -ceq "arg=$($Payload[$Index])") `
            "launch changed forwarded argument $Index"
    }

    $LifecycleLog = Join-Path $env:USERPROFILE "darpc\logs\pid-$($Result.pid).log"
    Wait-ForFile $LifecycleLog
    Assert-True `
        ($null -ne (Select-String -Path $LifecycleLog -Pattern "^event=initialized pid=$($Result.pid) version=")) `
        "launch did not produce the expected initialization log"

    $Result = Invoke-Loader -CommandArgs @("detach", "$($Process.Id)", $DarpcDll)
    Assert-True $Result.changed "launched target detach did not report a state change"
    Assert-TargetRunning $Process "launched target detach"
} finally {
    Stop-InjectionTarget $Process
}

Write-Host "Testing launch process exit"
$Result = Invoke-Loader -CommandArgs @(
    "launch",
    $LaunchTarget,
    $DarpcDll,
    "--",
    "--wait-ms",
    "50",
    "--launch-report",
    $LaunchExitReport
)
Assert-ProcessExited -ProcessId $Result.pid -Context "natural target exit"

Write-Host "Testing failed launch cleanup"
$PreviousMode = $env:DARPC_LOADER_TEST_MODE
try {
    $env:DARPC_LOADER_TEST_MODE = "init-fail"
    $Result = Invoke-Loader `
        -CommandArgs @(
            "launch",
            $LaunchTarget,
            $FixtureDarpcDll,
            "--",
            "--wait-ms",
            "30000",
            "--launch-report",
            $LaunchFailureReport
        ) `
        -ExpectedExitCode 11
} finally {
    if ($null -eq $PreviousMode) {
        Remove-Item Env:DARPC_LOADER_TEST_MODE -ErrorAction SilentlyContinue
    } else {
        $env:DARPC_LOADER_TEST_MODE = $PreviousMode
    }
}

Assert-True ($Result.error.kind -eq "initialization_failed") "failed launch error was not distinct"
Assert-True ($null -ne $Result.pid) "failed launch did not report its child PID"
Assert-ProcessExited -ProcessId $Result.pid -Context "failed launch cleanup"
Assert-True (-not (Test-Path $LaunchFailureReport)) "failed launch resumed the target"

Remove-Item -Recurse -Force $LaunchDirectory

Write-Host "Loader M3 integration checks passed"
exit 0
