param([string]$Package)
$ErrorActionPreference='Stop'
$repo=Split-Path $PSScriptRoot -Parent
$version=(Get-Content -LiteralPath (Join-Path $repo 'VERSION') -Raw).Trim()
if(!$Package){$Package=Join-Path $repo "dist/HITDROL-$($version.Replace(' ','-'))-windows-x64.msi"}
$Package=(Resolve-Path -LiteralPath $Package).Path
& (Join-Path $PSScriptRoot 'verify-installer-dialogs.ps1') -Package $Package
# Exercise the installed MSI tables and native feature/directory state after
# costing, without opening or automating msiexec windows or installing files.
Add-Type -TypeDefinition @'
using System;using System.Text;using System.Runtime.InteropServices;
public static class HitdrolMsiOptions {
 [DllImport("msi.dll",CharSet=CharSet.Unicode)] public static extern uint MsiOpenPackageEx(string path,uint options,out uint handle);
 [DllImport("msi.dll",CharSet=CharSet.Unicode)] public static extern uint MsiDoAction(uint handle,string action);
 [DllImport("msi.dll")] public static extern uint MsiSetInstallLevel(uint handle,int level);
 [DllImport("msi.dll",CharSet=CharSet.Unicode)] public static extern uint MsiGetFeatureState(uint handle,string name,out int installed,out int action);
 [DllImport("msi.dll",CharSet=CharSet.Unicode)] public static extern uint MsiSetFeatureState(uint handle,string name,int action);
 [DllImport("msi.dll",CharSet=CharSet.Unicode)] public static extern uint MsiSetProperty(uint handle,string name,string value);
 [DllImport("msi.dll",CharSet=CharSet.Unicode)] public static extern int MsiEvaluateCondition(uint handle,string condition);
 [DllImport("msi.dll",CharSet=CharSet.Unicode)] public static extern uint MsiSetTargetPath(uint handle,string folder,string path);
 [DllImport("msi.dll",CharSet=CharSet.Unicode)] public static extern uint MsiGetTargetPath(uint handle,string folder,StringBuilder path,ref uint length);
 [DllImport("msi.dll")] public static extern uint MsiCloseHandle(uint handle);
 [DllImport("msi.dll")] public static extern uint MsiSetInternalUI(uint level,IntPtr window);
}
'@
function Check([uint32]$Status){if($Status){throw "Native MSI call failed: $Status"}}
$installer=New-Object -ComObject WindowsInstaller.Installer
$db=$installer.OpenDatabase($Package,0)
$view=$db.OpenView('SELECT `Event`,`Argument`,`Condition` FROM `ControlEvent` WHERE `Dialog_` = ''OptionsDlg'' AND `Control_` = ''Install'' ORDER BY `Ordering`')
$view.Execute();$events=@()
while($record=$view.Fetch()){$events+=@{Event=$record.StringData(1);Argument=$record.StringData(2);Condition=$record.StringData(3)}}
$view.Close()
if($events.Event-contains'[ADDLOCAL]' -or $events.Event-notcontains'AddLocal' -or $events.Event-notcontains'SetTargetPath'){throw 'Installer still uses late property assignments instead of feature/path events'}
$view=$db.OpenView('SELECT `Type` FROM `Control` WHERE `Dialog_` = ''BrowseDlg''')
$view.Execute();$types=@();while($record=$view.Fetch()){$types+=$record.StringData(1)};$view.Close()
foreach($type in @('DirectoryCombo','DirectoryList','PathEdit')){if($type-notin$types){throw "Missing folder picker control: $type"}}
$selected=Join-Path $repo 'artifacts/installer-options/Другая папка/HITDROL'
$previousUi=[HitdrolMsiOptions]::MsiSetInternalUI(2,[IntPtr]::Zero)
try {
    foreach($startup in @(0,1)){foreach($desktop in @(0,1)){
        [uint32]$session=0
        Check ([HitdrolMsiOptions]::MsiOpenPackageEx($Package,1,[ref]$session))
        try {
            foreach($action in @('CostInitialize','FileCost','CostFinalize')){Check ([HitdrolMsiOptions]::MsiDoAction($session,$action))}
            Check ([HitdrolMsiOptions]::MsiSetInstallLevel($session,1))
            # The actual bug: checkbox and path are changed AFTER CostFinalize.
            Check ([HitdrolMsiOptions]::MsiSetProperty($session,'AUTOSTART',$(if($startup){'1'}else{''})))
            Check ([HitdrolMsiOptions]::MsiSetProperty($session,'DESKTOPSHORTCUT',$(if($desktop){'1'}else{''})))
            Check ([HitdrolMsiOptions]::MsiSetProperty($session,'INSTALLDIR',$selected))
            foreach($event in $events){
                $condition=[HitdrolMsiOptions]::MsiEvaluateCondition($session,$event.Condition)
                if($condition-eq3){throw 'Invalid installer event condition'}
                if($condition-ne1){continue}
                switch($event.Event){
                    'AddLocal' {Check ([HitdrolMsiOptions]::MsiSetFeatureState($session,$event.Argument,3))}
                    'Remove' {Check ([HitdrolMsiOptions]::MsiSetFeatureState($session,$event.Argument,2))}
                    'SetTargetPath' {Check ([HitdrolMsiOptions]::MsiSetTargetPath($session,$event.Argument,$selected))}
                    'EndDialog' {}
                    default {throw "Unverified install event: $($event.Event)"}
                }
            }
            foreach($feature in @('Main','AutoStart','Desktop')){
                [int]$installed=0;[int]$requested=0
                Check ([HitdrolMsiOptions]::MsiGetFeatureState($session,$feature,[ref]$installed,[ref]$requested))
                $expected=if($feature-eq'Main' -or ($feature-eq'AutoStart' -and $startup) -or ($feature-eq'Desktop' -and $desktop)){3}else{2}
                if($requested-ne$expected){throw "Late checkbox ignored: $feature = $requested, expected $expected"}
            }
            [uint32]$length=1024;$path=[Text.StringBuilder]::new(1024)
            Check ([HitdrolMsiOptions]::MsiGetTargetPath($session,'INSTALLDIR',$path,[ref]$length))
            if($path.ToString().TrimEnd('\')-ne$selected.TrimEnd('\')){throw 'Selected installation path ignored'}
            [pscustomobject]@{Startup=$startup;Desktop=$desktop;LateCheckboxesApplied=$true;SelectedPathApplied=$true}
        } finally {[void][HitdrolMsiOptions]::MsiCloseHandle($session)}
    }}
} finally {
    [void][HitdrolMsiOptions]::MsiSetInternalUI($previousUi,[IntPtr]::Zero)
    foreach($object in @($record,$view,$db,$installer)){if($object){[void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($object)}}
}
