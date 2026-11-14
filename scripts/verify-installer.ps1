param([string]$Package,[string]$PreviousPackage)
$ErrorActionPreference='Stop'
$repo=Split-Path $PSScriptRoot -Parent
$version=(Get-Content -LiteralPath (Join-Path $repo 'VERSION') -Raw).Trim()
if(!$Package){$Package=Join-Path $repo "dist/HITDROL-$($version.Replace(' ','-'))-windows-x64.msi"}
$Package=(Resolve-Path -LiteralPath $Package).Path
$qa=Join-Path $repo 'artifacts/installer-qa'
New-Item -ItemType Directory -Force -Path $qa|Out-Null
$cleanupExe=Join-Path $qa 'startup-cleanup.exe'
$installed=Join-Path $env:LOCALAPPDATA 'HITDROL-Installer-QA'
$alternate=Join-Path $qa 'Другой диск и папка/HITDROL'
$exe=Join-Path $installed 'HITDROL.exe'
$run='HKCU:/Software/Microsoft/Windows/CurrentVersion/Run'
$shell=New-Object -ComObject WScript.Shell
$desktop=Join-Path ($shell.SpecialFolders.Item('Desktop')) ("HITDROL $version.lnk")
$menu=Join-Path ($shell.SpecialFolders.Item('Programs')) ("HITDROL/HITDROL $version.lnk")
foreach($link in @($desktop,$menu)){if(Test-Path -LiteralPath $link){throw 'Existing HITDROL shortcut found; verification will not replace it'}}
function HasStartup{return $null-ne(Get-ItemProperty -LiteralPath $run -Name HITDROL -ErrorAction SilentlyContinue)}
function WindowsStartup{return [bool](Get-CimInstance Win32_StartupCommand|Where-Object Name -eq HITDROL)}
if(HasStartup){throw 'An existing HITDROL startup entry is present; verification will not replace it'}
foreach($folder in @($installed,$alternate)){if(Test-Path -LiteralPath (Join-Path $folder 'HITDROL.exe')){throw 'Installer QA target already contains HITDROL'}}
$installer=New-Object -ComObject WindowsInstaller.Installer
$db=$installer.OpenDatabase($Package,0)
$view=$db.OpenView('SELECT `Value` FROM `Property` WHERE `Property` = ''ProductCode''');$view.Execute();$row=$view.Fetch();$product=$row.StringData(1);$view.Close()
foreach($object in @($row,$view,$db)){[Runtime.InteropServices.Marshal]::FinalReleaseComObject($object)|Out-Null}
$row=$null;$view=$null;$db=$null;[GC]::Collect();[GC]::WaitForPendingFinalizers()
if($installer.ProductState($product)-eq5){throw 'A user installation is already registered'}
if(Test-Path -LiteralPath ('HKCU:/Software/Microsoft/Windows/CurrentVersion/Uninstall/'+$product)){throw 'HITDROL is already installed; verification will not replace a user installation'}
function Msi([string]$Arguments){
    $info=[Diagnostics.ProcessStartInfo]::new((Get-Command msiexec.exe).Source);$info.UseShellExecute=$false;$info.Arguments=$Arguments
    $process=[Diagnostics.Process]::Start($info);$process.WaitForExit()
    if($process.ExitCode){throw "MSI verification exit: $($process.ExitCode). See $qa logs"}
}
function App([string[]]$Arguments){
    $info=[Diagnostics.ProcessStartInfo]::new($exe);$info.UseShellExecute=$false
    foreach($arg in $Arguments){$info.ArgumentList.Add($arg)}
    $process=[Diagnostics.Process]::Start($info)
    if(!$process.WaitForExit(15000)){throw 'Installed app verification timed out'}
    if($process.ExitCode){throw 'Installed app failed'}
}
$profile=Join-Path $env:LOCALAPPDATA 'HITDROLLauncher/library.db'
$before=if(Test-Path -LiteralPath $profile){(Get-FileHash -LiteralPath $profile).Hash}else{$null}
$results=@()
foreach($enabled in @(0,1)){
    $installed=if($enabled){$alternate}else{Join-Path $env:LOCALAPPDATA 'HITDROL-Installer-QA'}
    $exe=Join-Path $installed 'HITDROL.exe'
    Msi ('/i "'+$Package+'" /qn /norestart /L*v "'+(Join-Path $qa "final-install-$enabled.log")+'" INSTALLDIR="'+$installed+'" AUTOSTART='+$enabled+' DESKTOPSHORTCUT='+$enabled)
    if((WindowsStartup)-ne[bool]$enabled){throw 'MSI startup selection mismatch in Windows'}
    if((Test-Path -LiteralPath $desktop)-ne[bool]$enabled){throw 'Desktop shortcut selection mismatch'}
    foreach($link in @($menu)+$(if($enabled){@($desktop)}else{@()})){
        if(!(Test-Path -LiteralPath $link) -or $shell.CreateShortcut($link).TargetPath-ne$exe){throw 'Invalid installed shortcut'}
    }
    App @('--enable-autostart');if(!(HasStartup)){throw 'App startup enable failed'}
    $command=(Get-ItemProperty -LiteralPath $run -Name HITDROL).HITDROL
    if($command-ne('"'+$exe+'" --autostart')){throw 'Incorrect startup command'}
    $startupVisible=WindowsStartup
    App @('--disable-autostart');if(HasStartup){throw 'App startup disable failed'}
    App @('--enable-autostart')
    if($enabled-eq0){
        $data=Join-Path $qa 'installed-app-profile'
        App @('--demo','12','--windowed','--size','1280x800','--page','settings','--data-dir',$data,'--screenshot',(Join-Path $qa 'installed-settings-final.png'),'--quit-after','4')
        $report=Get-Content -LiteralPath (Join-Path $data 'last-run.json') -Raw|ConvertFrom-Json
        if($report.error-or!$report.capture){throw 'Installed graphics smoke test failed'}
    }
    # A packaged development host can redirect child-process registry writes.
    # WMI reads the system view used by Windows Installer and Task Manager.
    $isolated=!(WindowsStartup)
    if($isolated){App @('--disable-autostart')}
    Copy-Item -LiteralPath $exe -Destination $cleanupExe -Force
    Msi ('/x '+$product+' /qn /norestart /L*v "'+(Join-Path $qa "final-uninstall-$enabled.log")+'"')
    if(WindowsStartup){throw 'Uninstall left a Windows startup entry'}
    if(HasStartup){
        # Remove only our development-host test entry, invisible to MSI/WMI.
        $info=[Diagnostics.ProcessStartInfo]::new($cleanupExe);$info.UseShellExecute=$false;$info.ArgumentList.Add('--disable-autostart')
        $cleanup=[Diagnostics.Process]::Start($info)
        if(!$cleanup.WaitForExit(15000) -or $cleanup.ExitCode){throw 'Development-host startup cleanup process failed'}
        if(HasStartup){throw 'Development-host startup cleanup failed'}
    }
    if(Test-Path -LiteralPath $exe){throw 'Uninstall left the application executable'}
    if((Test-Path -LiteralPath $desktop)-or(Test-Path -LiteralPath $menu)){throw 'Uninstall left a shortcut'}
    if($before-and(Get-FileHash -LiteralPath $profile).Hash-ne$before){throw 'Uninstall changed the real user library'}
    Remove-Item -LiteralPath $cleanupExe -Force
    if($enabled-eq1 -and !(Select-String -LiteralPath (Join-Path $qa "final-uninstall-$enabled.log") -SimpleMatch 'HITDROL startup cleanup result: 0')){throw 'Native uninstall cleanup did not remove the real startup value'}
    $result=[pscustomobject]@{Version=$version;InstallerAutostart=$enabled;SelectedFolder=$installed;AppStartupControls=$true;WindowsStartupVisible=$startupVisible;DevelopmentRegistryIsolated=$isolated;UninstallStartupCleanup=$true;Shortcuts=$true;UserLibraryPreserved=$true}
    $results+=$result;$result
}
if($PreviousPackage){
    $PreviousPackage=(Resolve-Path -LiteralPath $PreviousPackage).Path
    $db=$installer.OpenDatabase($PreviousPackage,0)
    $view=$db.OpenView('SELECT `Value` FROM `Property` WHERE `Property` = ''ProductCode''');$view.Execute();$row=$view.Fetch();$previous=$row.StringData(1);$view.Close()
    foreach($object in @($row,$view,$db)){[void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($object)}
    $row=$null;$view=$null;$db=$null;[GC]::Collect();[GC]::WaitForPendingFinalizers()
    if($installer.ProductState($previous)-eq5){throw 'Previous user installation already exists; upgrade test will not replace it'}
    $oldFolder=Join-Path $env:LOCALAPPDATA 'HITDROL-Installer-QA'
    $installed=$alternate;$exe=Join-Path $installed 'HITDROL.exe'
    Msi ('/i "'+$PreviousPackage+'" /qn /norestart /L*v "'+(Join-Path $qa 'upgrade-old.log')+'" INSTALLDIR="'+$oldFolder+'" AUTOSTART=1 DESKTOPSHORTCUT=0')
    Msi ('/i "'+$Package+'" /qn /norestart /L*v "'+(Join-Path $qa 'upgrade-new.log')+'" INSTALLDIR="'+$installed+'" AUTOSTART=1 DESKTOPSHORTCUT=0')
    $startup=@(Get-CimInstance Win32_StartupCommand|Where-Object Name -eq HITDROL)
    if($startup.Count-ne1 -or $startup[0].Command-ne('"'+$exe+'" --autostart')){throw 'Upgrade removed or kept a stale Windows startup command'}
    if(Test-Path -LiteralPath (Join-Path $oldFolder 'HITDROL.exe')){throw 'Old executable remains after moving the installation'}
    if(!(Test-Path -LiteralPath $exe) -or $installer.ProductState($previous)-eq5){throw 'Upgrade did not replace the old product'}
    if($shell.CreateShortcut($menu).TargetPath-ne$exe){throw 'Upgrade left a stale Start menu shortcut'}
    Msi ('/x '+$product+' /qn /norestart /L*v "'+(Join-Path $qa 'upgrade-uninstall.log')+'"')
    if((WindowsStartup)-or(Test-Path -LiteralPath $exe)){throw 'Upgraded product did not uninstall cleanly'}
    if($before-and(Get-FileHash -LiteralPath $profile).Hash-ne$before){throw 'Upgrade changed user library'}
    $result=[pscustomobject]@{Version=$version;UpgradeFrom=$previous;ChangedFolder=$true;WindowsStartupPreserved=$true;UserLibraryPreserved=$true}
    $results+=$result;$result
}
$results|ConvertTo-Json|Set-Content -LiteralPath (Join-Path $qa 'summary.json') -Encoding utf8
