param([string]$Package)
$ErrorActionPreference='Stop'
$repo=Split-Path $PSScriptRoot -Parent
if(!$Package){$Package=Join-Path $repo 'dist/HITDROL-Beta-0.1-windows-x64.msi'}
$installer=New-Object -ComObject WindowsInstaller.Installer
$db=$installer.OpenDatabase((Resolve-Path -LiteralPath $Package).Path,0)
function ReadRow([string]$Sql,[int]$Fields){
    $view=$db.OpenView($Sql);$view.Execute()|Out-Null;$record=$view.Fetch()
    if(!$record){throw "Missing MSI row: $Sql"}
    $result=@();for($i=1;$i-le$Fields;$i++){$result+=$record.StringData($i)}
    $view.Close()|Out-Null;return ,$result
}
try {
    $view=$db.OpenView('SELECT `Action`,`Target` FROM `CustomAction`');$view.Execute()
    while($record=$view.Fetch()){
        if($record.StringData(1)-match 'ConfigureEarly|Create.*Task' -or $record.StringData(2)-match 'ConfigureEarly|--early-autostart|schtasks'){throw 'Retired early startup action is present'}
    }
    $view.Close()
    $action=ReadRow 'SELECT `Type`,`Source`,`Target` FROM `CustomAction` WHERE `Action`=''RemoveLegacyStartupTask''' 3
    if($action[0]-ne'1'-or$action[1]-ne'RegistryCleanup'-or$action[2]-ne'RemoveLegacyStartupTask'){throw 'Missing legacy-task cleanup'}
    $configure=ReadRow 'SELECT `Condition`,`Sequence` FROM `InstallExecuteSequence` WHERE `Action`=''RemoveLegacyStartupTask''' 2
    $commit=ReadRow 'SELECT `Sequence` FROM `InstallExecuteSequence` WHERE `Action`=''InstallFinalize''' 1
    if([int]$configure[1]-le[int]$commit[0]-or$configure[0]-ne'NOT (REMOVE ~= "ALL")'){throw 'Legacy cleanup must run after committed installation'}
    $cleanup=ReadRow 'SELECT `Condition`,`Sequence` FROM `InstallExecuteSequence` WHERE `Action`=''UninstallStartupRows''' 2
    $remove=ReadRow 'SELECT `Sequence` FROM `InstallExecuteSequence` WHERE `Action`=''RemoveFiles''' 1
    if($cleanup[0]-ne'REMOVE ~= "ALL"'-or[int]$cleanup[1]-ge[int]$remove[0]){throw 'Startup cleanup must run before files are removed'}
    $run=ReadRow 'SELECT `Root`,`Key`,`Name`,`Value` FROM `Registry` WHERE `Registry`=''StartupEntry''' 4
    if($run[0]-ne'1'-or$run[1]-ne'Software\Microsoft\Windows\CurrentVersion\Run'-or$run[2]-ne'HITDROL'-or$run[3]-ne'"[INSTALLDIR]HITDROL.exe" --autostart'){throw 'Visible per-user startup switch was changed'}
    $feature=ReadRow 'SELECT `Level` FROM `Feature` WHERE `Feature`=''AutoStart''' 1
    if($feature[0]-ne'200'){throw 'Autostart must be off by default'}
    $property=$db.OpenView('SELECT `Value` FROM `Property` WHERE `Property`=''AUTOSTART''');$property.Execute()
    if($property.Fetch()){throw 'Autostart must require an explicit choice'};$property.Close()
    [pscustomobject]@{EarlyStartupRemoved=$true;LegacyTaskCleanup=$true;AutostartOffByDefault=$true;CleanupBeforeFileRemoval=$true;TaskManagerSwitchRetained=$true}|ConvertTo-Json
} finally {
    [Runtime.InteropServices.Marshal]::FinalReleaseComObject($db)|Out-Null
    [Runtime.InteropServices.Marshal]::FinalReleaseComObject($installer)|Out-Null
}
