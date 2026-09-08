param(
    [Parameter(Mandatory = $true)] [string] $Client,
    [Parameter(Mandatory = $true)] [string] $Merchant,
    [string] $Choice = 'Withdraw Item',
    [string] $BaseUrl = 'http://127.0.0.1:2626'
)

# Opens item menus and submits no-op diagnostics. Does not select an item.
$ErrorActionPreference = 'Stop'
$Url = "$BaseUrl/clients/$([uri]::EscapeDataString($Client))"

function Request($Path, $Body, $Method = 'Post') {
    Invoke-RestMethod -Method $Method -ContentType 'application/json' `
        -Body ($Body | ConvertTo-Json -Compress) -Uri "$Url/$Path"
}

function Wait-Dialog($Type, $PreviousRevision) {
    for ($Attempt = 0; $Attempt -lt 40; $Attempt++) {
        $Page = (Invoke-RestMethod "$Url/dialog").dialog
        if ($Page -and $Page.revision -ne $PreviousRevision -and
            -not $Page.response_pending -and $Page.interaction.type -eq $Type) {
            return $Page
        }
        Start-Sleep -Milliseconds 100
    }
    throw "Timed out waiting for a new $Type dialog"
}

function Open-Items {
    $Current = (Invoke-RestMethod "$Url/dialog").dialog
    if ($Current) { $null = Request 'dialog/close' @{revision = $Current.revision} }
    $null = Request 'interact' @{target = $Merchant}
    $Page = Wait-Dialog 'choices' $Current.revision
    $Entry = @($Page.interaction.data | Where-Object text -EQ $Choice)
    if ($Entry.Count -ne 1) { throw 'Expected exactly one matching menu choice' }
    $null = Request 'dialog/select' @{revision = $Page.revision; index = $Entry[0].index}
    $Items = Wait-Dialog 'items' $Page.revision
    return $Items
}

$InitialMode = (Invoke-RestMethod "$Url/diagnostics/hooks").mode
$null = Request 'diagnostics' @{mode = 'hook_timing'; reset = $true} 'Put'
try {
    $Snapshot = Invoke-RestMethod "$Url/dialog"
    $ProcessId = $Snapshot.observation.pid
    $Log = Join-Path $env:USERPROFILE "darpc/logs/pid-$ProcessId.log"
    $LogBefore = if (Test-Path $Log) { (Get-Item $Log).Length } else { 0 }
    $CpuBefore = (Get-Process -Id $ProcessId).CPU
    $Clock = [Diagnostics.Stopwatch]::StartNew()
    $Delays = @()
    $Timeouts = 0
    for ($Index = 0; $Index -lt 60; $Index++) {
        if ($Index % 20 -eq 0) { $Items = Open-Items }
        $Result = Request 'commands/diagnostic' @{}
        if ($Result.state -ne 'executed') { $Timeouts++ }
        if ($null -ne $Result.queue_delay_ms) { $Delays += $Result.queue_delay_ms }
        Start-Sleep -Milliseconds 200
    }
    $Clock.Stop()
    $Timings = (Invoke-RestMethod "$Url/diagnostics/hooks").hook_timings
    $Sorted = @($Delays | Sort-Object)
    if ($Sorted.Count -ne 60) { throw 'Missing diagnostic queue-delay samples' }
    $LogAfter = if (Test-Path $Log) { (Get-Item $Log).Length } else { 0 }
    [pscustomobject]@{
        samples = $Sorted.Count
        queue_p50_ms = $Sorted[29]
        queue_p95_ms = $Sorted[56]
        queue_max_ms = $Sorted[-1]
        nonexecuted_commands = $Timeouts
        elapsed_seconds = [math]::Round($Clock.Elapsed.TotalSeconds, 3)
        cpu_seconds = [math]::Round((Get-Process -Id $ProcessId).CPU - $CpuBefore, 3)
        log_growth_bytes = $LogAfter - $LogBefore
        item_count = $Items.interaction.data.Count
        tick = $Timings | Where-Object stage -EQ 'tick'
        event = $Timings | Where-Object stage -EQ 'event'
    } | ConvertTo-Json -Depth 5
} finally {
    $null = Request 'diagnostics' @{mode = $InitialMode} 'Put'
}
