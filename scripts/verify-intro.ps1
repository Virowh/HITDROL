param([string]$Executable,[string[]]$CaseName)
$ErrorActionPreference='Stop'
$repo=Split-Path $PSScriptRoot -Parent
if (!$Executable) {$Executable=Join-Path $repo 'target/release/hitdrol-launcher.exe'}
$Executable=(Resolve-Path -LiteralPath $Executable).Path
$root=Join-Path $repo 'artifacts/intro-qa'
New-Item -ItemType Directory -Force -Path $root | Out-Null
$cases=@(
    @{Name='strike-1280x720';Size='1280x720';At='1.42'},
    @{Name='strike-1280x800';Size='1280x800';At='1.42'},
    @{Name='strike-1920x1080';Size='1920x1080';At='1.42'},
    @{Name='strike-1920x1200';Size='1920x1200';At='1.42'},
    @{Name='motion-30';Size='1280x800';Fps=30},
    @{Name='motion-60';Size='1280x800';Fps=60},
    @{Name='motion-fullscreen';Size='1280x800';Fps=60;Fullscreen=$true},
    @{Name='live-repeat-1';Size='1280x800';Repeat=$true},
    @{Name='live-repeat-2';Size='1280x800';Repeat=$true},
    @{Name='live-repeat-3';Size='1280x800';Repeat=$true},
    @{Name='disabled';Size='1280x720';Disabled=$true}
)
$results=@()
if($CaseName){$cases=@($cases|Where-Object {$_.Name-in$CaseName});if(!$cases.Count){throw 'Unknown intro case'}}
foreach ($case in $cases) {
    $data=Join-Path $root $case.Name
    $capture=Join-Path $root ($case.Name+'.png')
    $info=[Diagnostics.ProcessStartInfo]::new($Executable)
    $info.UseShellExecute=$false
    $duration=if($case.Fps -or $case.Repeat){'6'}else{'3'}
    foreach($arg in @('--demo','12','--size',$case.Size,'--data-dir',$data,'--screenshot',$capture,'--quit-after',$duration)) {$info.ArgumentList.Add($arg)}
    if(!$case.Fullscreen){$info.ArgumentList.Add('--windowed')}
    if($case.At){$info.ArgumentList.Add('--intro-at');$info.ArgumentList.Add($case.At)}
    if($case.Fps){foreach($arg in @('--intro','--fps',"$($case.Fps)",'--benchmark')){$info.ArgumentList.Add($arg)}}
    if($case.Disabled){$info.ArgumentList.Add('--no-intro')}
    if($case.Repeat){foreach($arg in @('--intro','--screenshot-after','1.5')){$info.ArgumentList.Add($arg)}}
    $process=[Diagnostics.Process]::Start($info)
    if(!$process.WaitForExit(15000)){throw "Intro timed out: $($case.Name)"}
    if($process.ExitCode){throw "Intro failed: $($case.Name)"}
    $report=Get-Content -LiteralPath (Join-Path $data 'last-run.json') -Raw | ConvertFrom-Json
    if($report.error -or !$report.capture -or !(Test-Path -LiteralPath $capture)){throw "Intro capture failed: $($case.Name)"}
    if($case.Disabled -and $report.intro_enabled){throw 'Intro was not disabled'}
    if($case.At -or $case.Repeat){
        if($case.Repeat -and ($report.first_frame_seconds-gt1.1 -or $report.intro_skipped -or $report.intro_duration_seconds-lt2.5)){throw 'Cannot verify live intro capture timing'}
        $bitmap=[Drawing.Bitmap]::new($capture)
        try {
            $blue=0
            for($y=0;$y-lt$bitmap.Height;$y+=8){for($x=0;$x-lt$bitmap.Width;$x+=8){
                $pixel=$bitmap.GetPixel($x,$y)
                if($pixel.B-gt80 -and $pixel.G-gt50 -and $pixel.B-gt($pixel.R*1.2)){$blue++}
            }}
            if($blue-lt100){throw "Intro is black or its icy layers are missing: $($case.Name)"}
        } finally {$bitmap.Dispose()}
    }
    $minimum=$null
    if($case.Fps) {
        $minimum=($report.frame_intervals_ms|Select-Object -Skip 3|Measure-Object -Minimum).Minimum
        if(!$report.intro_enabled -or $report.intro_skipped -or $report.intro_duration_seconds -lt 2.5 -or $report.intro_duration_seconds -gt 2.65){throw "Intro duration failed: $($case.Name)"}
        if($minimum -lt (1000.0/$case.Fps-0.01)){throw "Intro FPS cap failed: $($case.Name)"}
    }
    $result=[pscustomobject]@{Case=$case.Name;Version=$report.version;IntroSeconds=$report.intro_duration_seconds;FirstFrameSeconds=$report.first_frame_seconds;MinFrameMs=$minimum;Error=$report.error}
    $results+=$result
    $result
}
$results|ConvertTo-Json -Depth 5|Set-Content -LiteralPath (Join-Path $root 'summary.json') -Encoding utf8
