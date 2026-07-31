param(
    [Parameter(Mandatory = $true)]
    [string] $TargetDir
)

$ErrorActionPreference = "Stop"

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

Write-Host "Loader M2 integration checks passed"
