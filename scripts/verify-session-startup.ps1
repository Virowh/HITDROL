param([string]$Executable)
$ErrorActionPreference='Stop'
$repo=Split-Path $PSScriptRoot -Parent
if(!$Executable){$Executable=Join-Path $repo 'target/release/hitdrol-launcher.exe'}
$Executable=(Resolve-Path -LiteralPath $Executable).Path
$qa=Join-Path $repo ('artifacts/session-startup/'+[guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Force -Path $qa|Out-Null
Add-Type -TypeDefinition @'
using System;using System.Text;using System.Runtime.InteropServices;
public static class HitdrolStartupVisibility {
 delegate bool Callback(IntPtr h,IntPtr p);
 [DllImport("user32.dll")] static extern bool EnumWindows(Callback cb,IntPtr p);
 [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr h,out uint p);
 [DllImport("user32.dll",CharSet=CharSet.Unicode)] static extern int GetWindowText(IntPtr h,StringBuilder text,int length);
 [DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr h);
 public static bool Visible(int pid) {
  bool visible=false;
  EnumWindows((h,p)=>{uint owner;GetWindowThreadProcessId(h,out owner);if(owner==pid){var title=new StringBuilder(256);GetWindowText(h,title,256);if(title.ToString().StartsWith("HITDROL")&&IsWindowVisible(h))visible=true;}return true;},IntPtr.Zero);
  return visible;
 }
}
'@
$info=[Diagnostics.ProcessStartInfo]::new($Executable);$info.UseShellExecute=$false
foreach($arg in @('--demo','12','--windowed','--size','1280x800','--autostart','--startup-delay','3','--intro','--data-dir',$qa,'--screenshot',(Join-Path $qa 'after-unlock.png'),'--screenshot-after','9','--quit-after','11')){$info.ArgumentList.Add($arg)}
$process=[Diagnostics.Process]::Start($info)
Start-Sleep -Milliseconds 1000
$process.Refresh()
if($process.HasExited){throw 'Startup waiting process exited early'}
# winit's empty internal event-target window is visible to EnumWindows but has
# no launcher content. Check the application's titled window, not .NET's guess.
if([HitdrolStartupVisibility]::Visible($process.Id)){throw 'Launcher window appeared before the startup gate opened'}
if(Test-Path -LiteralPath (Join-Path $qa 'library.db')){throw 'Database opened before the startup gate'}
$secondInfo=[Diagnostics.ProcessStartInfo]::new($Executable);$secondInfo.UseShellExecute=$false
foreach($arg in @('--demo','12','--windowed','--data-dir',$qa,'--quit-after','2')){$secondInfo.ArgumentList.Add($arg)}
$second=[Diagnostics.Process]::Start($secondInfo)
if(!$second.WaitForExit(1500)-or$second.ExitCode){throw 'A duplicate startup process did not exit before creating another window'}
if(!$process.WaitForExit(20000)){throw 'Startup gate did not release on the interactive desktop'}
if($process.ExitCode){throw 'Deferred startup failed'}
$report=Get-Content -LiteralPath (Join-Path $qa 'last-run.json') -Raw|ConvertFrom-Json
if($report.error-or!$report.capture-or!$report.autostart-or$report.session_wait_seconds-lt5-or$report.first_frame_seconds-lt5){throw 'Session gate or stable-session wait failed'}
if($report.graphics_before_session-ne$false){throw 'Graphics initialized before the session gate'}
$afterReady=$report.first_frame_seconds-$report.session_wait_seconds
if($afterReady-lt0-or$afterReady-gt10){throw 'Invalid initialization time after session readiness'}
if($report.intro_foreground_denied){
    if(!$report.intro_skipped -or $null-ne$report.intro_duration_seconds){throw 'Background startup did not skip the intro cleanly'}
} elseif($report.intro_skipped-or$null-eq$report.intro_duration_seconds-or$report.intro_duration_seconds-lt2.5-or$report.intro_duration_seconds-gt2.7){throw 'The waiting period consumed the startup animation'}
[pscustomobject]@{Build=$report.build_version;NoVisibleWindowWhileWaiting=$true;NoDatabaseWhileWaiting=$true;GraphicsBeforeSession=$report.graphics_before_session;DuplicateProcessExited=$true;SessionWaitSeconds=$report.session_wait_seconds;FirstFrameSeconds=$report.first_frame_seconds;AfterReadySeconds=$afterReady;IntroSeconds=$report.intro_duration_seconds;IntroSkippedWithoutForeground=$report.intro_foreground_denied;Error=$report.error;ReportFolder=$qa}|ConvertTo-Json
