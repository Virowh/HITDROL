param([string]$Executable, [switch]$SkipBenchmarks, [string]$CaseFilter='*')
$ErrorActionPreference = 'Stop'
$repo = Split-Path $PSScriptRoot -Parent
if (!$Executable) { $Executable = Join-Path $repo 'target/release/hitdrol-launcher.exe' }
$Executable = (Resolve-Path -LiteralPath $Executable).Path
$results = Join-Path $repo 'artifacts/release-qa'
New-Item -ItemType Directory -Force -Path $results | Out-Null
$cases = @(
    @{ Name='home-1280x720'; Size='1280x720'; Page='home' },
    @{ Name='home-1280x800'; Size='1280x800'; Page='home' },
    @{ Name='library-1920x1080'; Size='1920x1080'; Page='library'; Count=1000 },
    @{ Name='detail-1920x1200'; Size='1920x1200'; Page='detail' },
    @{ Name='keyboard-150pct'; Size='1280x720'; Page='keyboard'; Scale='1.5' },
    @{ Name='settings-200pct'; Size='1920x1200'; Page='settings'; Scale='2' },
    @{ Name='add-1280x720'; Size='1280x720'; Page='add' },
    @{ Name='browser-1280x720'; Size='1280x720'; Page='browser' },
    @{ Name='empty-1280x720'; Size='1280x720'; Page='empty' },
    @{ Name='library-mode-1280x720'; Size='1280x720'; Page='all-settings' },
    @{ Name='install-1280x800'; Size='1280x800'; Page='install' },
    @{ Name='account-150pct'; Size='1280x720'; Page='account'; Scale='1.5' },
    @{ Name='no-history-1280x720'; Size='1280x720'; Page='recent-empty' }
)
if (!$SkipBenchmarks) {
    foreach ($rate in @(30,40,60,90,120,144,240)) {
        $cases += @{ Name="fps-$rate"; Size='1280x800'; Page='library'; Count=1000; Fps=$rate; Benchmark=$true }
    }
}
$summary = @()
if ($CaseFilter -ne '*' -and (Test-Path -LiteralPath (Join-Path $results 'summary.json'))) {
    $summary = @(Get-Content -Raw -LiteralPath (Join-Path $results 'summary.json') | ConvertFrom-Json | Where-Object Case -NotLike $CaseFilter)
}
foreach ($case in $cases) {
    if ($case.Name -notlike $CaseFilter) { continue }
    $data = Join-Path $results $case.Name
    $capture = Join-Path $results ($case.Name + '.png')
    $info = [Diagnostics.ProcessStartInfo]::new($Executable)
    $info.UseShellExecute = $false
    $count = if ($case.ContainsKey('Count')) { $case['Count'] } else { 12 }
    foreach ($arg in @('--demo', "$count", '--windowed', '--size', $case.Size, '--page', $case.Page,
            '--data-dir', $data, '--screenshot', $capture, '--quit-after', '5')) { $info.ArgumentList.Add($arg) }
    if ($case.Scale) { $info.ArgumentList.Add('--scale'); $info.ArgumentList.Add($case.Scale) }
    if ($case.Fps) { $info.ArgumentList.Add('--fps'); $info.ArgumentList.Add("$($case.Fps)") }
    if ($case.Benchmark) { $info.ArgumentList.Add('--benchmark') }
    $process = [Diagnostics.Process]::Start($info)
    Start-Sleep -Milliseconds 2500
    $process.Refresh()
    $cpuStart = $process.TotalProcessorTime.TotalMilliseconds
    $sample = [Diagnostics.Stopwatch]::StartNew()
    $memory = $process.WorkingSet64 / 1MB
    $gpu = $null
    try {
        $engines = Get-CimInstance Win32_PerfFormattedData_GPUPerformanceCounters_GPUEngine -Filter "Name LIKE 'pid_$($process.Id)_%'" -ErrorAction Stop
        if ($engines) { $gpu = ($engines | Where-Object Name -Like '*engtype_3D*' | Measure-Object UtilizationPercentage -Sum).Sum }
    } catch { }
    $process.WaitForExit(15000) | Out-Null
    if (!$process.HasExited) { throw "QA timed out: $($case.Name) (PID $($process.Id))" }
    $elapsed = $sample.Elapsed.TotalMilliseconds
    $cpuPercent = 100 * ($process.TotalProcessorTime.TotalMilliseconds - $cpuStart) / $elapsed
    $report = Get-Content -Raw -LiteralPath (Join-Path $data 'last-run.json') | ConvertFrom-Json
    if ($report.error -or !$report.capture -or !(Test-Path -LiteralPath $capture)) { throw "QA failed: $($case.Name)" }
    $intervals = @($report.frame_intervals_ms | Select-Object -Skip 3 | Sort-Object)
    $fastest = if ($intervals.Count) { $intervals[0] } else { $null }
    $median = if ($intervals.Count) { $intervals[[int]($intervals.Count / 2)] } else { $null }
    if ($case.Benchmark -and $fastest -lt (1000.0 / $case.Fps - 0.01)) { throw "FPS cap violated: $($case.Name) $fastest ms" }
    $summary += [pscustomobject]@{
        Case=$case.Name; FirstFrameSeconds=$report.first_frame_seconds; Frames=$report.frames;
        WorkingSetMiB=[math]::Round($memory,1); CpuPercentOneCore=[math]::Round($cpuPercent,2);
        Gpu3dPercent=$gpu; MinFrameMs=$fastest; MedianFrameMs=$median; Adapter=$report.adapter
    }
    Write-Output $summary[-1]
    $summary | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath (Join-Path $results 'summary.json') -Encoding utf8
}
