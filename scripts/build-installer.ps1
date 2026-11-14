param([switch]$SkipChecks,[switch]$SkipPackage)
$ErrorActionPreference='Stop'
$repo=Split-Path $PSScriptRoot -Parent
Push-Location $repo
try {
    if(!$SkipPackage){& (Join-Path $PSScriptRoot 'package.ps1') -SkipChecks:$SkipChecks}
    cargo build --manifest-path installer-actions/Cargo.toml --release --locked --target-dir target/installer-actions
    if($LASTEXITCODE){throw 'MSI registry action build failed'}
    $version=((Get-Content -Raw Cargo.toml|Select-String '(?m)^version = "([^"]+)"').Matches.Groups[1].Value)
    $displayVersion=(Get-Content -LiteralPath (Join-Path $repo 'VERSION') -Raw).Trim()
    $release=$displayVersion.Replace(' ','-')
    $productName="HITDROL $displayVersion"
    $zip=Join-Path $repo "dist/HITDROL-$release-windows-x64.zip"
    $stage=Join-Path $repo ('artifacts/installer-build-'+[guid]::NewGuid().ToString('N'))
    $payload=Join-Path $stage 'payload'
    $cabdir=Join-Path $stage 'cabinet'
    New-Item -ItemType Directory -Force -Path $payload,$cabdir|Out-Null
    [IO.Compression.ZipFile]::ExtractToDirectory($zip,$payload)
    $app=Join-Path $payload "HITDROL-$release-windows-x64"
    $msi=Join-Path $repo "dist/HITDROL-$release-windows-x64.msi"
    if(Test-Path -LiteralPath $msi){Remove-Item -LiteralPath $msi}
    $installer=New-Object -ComObject WindowsInstaller.Installer
    $db=$installer.OpenDatabase($msi,3)
    # Localize before inserting any text; all database and summary strings use CP1251.
    [IO.File]::WriteAllText((Join-Path $stage 'codepage.idt'),"`r`n`r`n1251`t_ForceCodepage`r`n",[Text.Encoding]::ASCII)
    $db.Import($stage,'codepage.idt')
    function Exec([string]$Sql){try{$v=$db.OpenView($Sql);$v.Execute();$v.Close()}catch{$errorRecord=$installer.LastErrorRecord();$detail=if($errorRecord){$errorRecord.FormatText()}else{$_};throw "MSI SQL: $Sql`n$detail"}}
    $tableColumns=@{}
    function Table([string]$Name,[string]$Columns,[string]$Keys){Exec ('CREATE TABLE `{0}` ({1} PRIMARY KEY {2})' -f $Name,$Columns,$Keys);$tableColumns[$Name]=@([regex]::Matches($Columns,'`([^`]+)`')|ForEach-Object{$_.Value})}
    function Row([string]$Name,[object[]]$Values){
        $params=($Values|ForEach-Object{'?'})-join ','
        $sql=('INSERT INTO `{0}` ({1}) VALUES ({2})' -f $Name,($tableColumns[$Name]-join ','),$params)
        try{$v=$db.OpenView($sql)}catch{$record=$installer.LastErrorRecord();throw "Insert $Name : $sql`n$($record.FormatText())"}
        $record=$installer.CreateRecord($Values.Count)
        for($i=0;$i-lt$Values.Count;$i++){
            $value=$Values[$i]
            if($null-eq$value){continue}
            if($value-is[int]){$record.IntegerData($i+1)=$value}else{$record.StringData($i+1)=[string]$value}
        }
        $v.Execute($record);$v.Close()
    }
    function Id([string]$Prefix,[string]$Value){$hash=[Convert]::ToHexString([Security.Cryptography.SHA256]::HashData([Text.Encoding]::UTF8.GetBytes($Value)));return $Prefix+$hash.Substring(0,24)}
    function GuidFor([string]$Value){$bytes=[Security.Cryptography.SHA256]::HashData([Text.Encoding]::UTF8.GetBytes($Value));return ([guid]::new([byte[]]$bytes[0..15])).ToString('B').ToUpperInvariant()}
    Table 'Property' '`Property` CHAR(72) NOT NULL, `Value` CHAR(0) NOT NULL LOCALIZABLE' '`Property`'
    Table 'Directory' '`Directory` CHAR(72) NOT NULL, `Directory_Parent` CHAR(72), `DefaultDir` CHAR(255) NOT NULL LOCALIZABLE' '`Directory`'
    Table 'Component' '`Component` CHAR(72) NOT NULL, `ComponentId` CHAR(38), `Directory_` CHAR(72) NOT NULL, `Attributes` SHORT NOT NULL, `Condition` CHAR(255), `KeyPath` CHAR(72)' '`Component`'
    Table 'Feature' '`Feature` CHAR(38) NOT NULL, `Feature_Parent` CHAR(38), `Title` CHAR(64) LOCALIZABLE, `Description` CHAR(255) LOCALIZABLE, `Display` SHORT, `Level` SHORT NOT NULL, `Directory_` CHAR(72), `Attributes` SHORT NOT NULL' '`Feature`'
    Table 'FeatureComponents' '`Feature_` CHAR(38) NOT NULL, `Component_` CHAR(72) NOT NULL' '`Feature_`, `Component_`'
    Table 'File' '`File` CHAR(72) NOT NULL, `Component_` CHAR(72) NOT NULL, `FileName` CHAR(255) NOT NULL LOCALIZABLE, `FileSize` LONG NOT NULL, `Version` CHAR(72), `Language` CHAR(20), `Attributes` SHORT, `Sequence` LONG NOT NULL' '`File`'
    Table 'Media' '`DiskId` SHORT NOT NULL, `LastSequence` LONG NOT NULL, `DiskPrompt` CHAR(64) LOCALIZABLE, `Cabinet` CHAR(255), `VolumeLabel` CHAR(32), `Source` CHAR(72)' '`DiskId`'
    Table 'Registry' '`Registry` CHAR(72) NOT NULL, `Root` SHORT NOT NULL, `Key` CHAR(255) NOT NULL, `Name` CHAR(255) LOCALIZABLE, `Value` CHAR(0) LOCALIZABLE, `Component_` CHAR(72) NOT NULL' '`Registry`'
    Table 'RemoveRegistry' '`RemoveRegistry` CHAR(72) NOT NULL, `Root` SHORT NOT NULL, `Key` CHAR(255) NOT NULL, `Name` CHAR(255) LOCALIZABLE, `Component_` CHAR(72) NOT NULL' '`RemoveRegistry`'
    Table 'Shortcut' '`Shortcut` CHAR(72) NOT NULL, `Directory_` CHAR(72) NOT NULL, `Name` CHAR(128) NOT NULL LOCALIZABLE, `Component_` CHAR(72) NOT NULL, `Target` CHAR(255) NOT NULL, `Arguments` CHAR(255), `Description` CHAR(255) LOCALIZABLE, `Hotkey` SHORT, `Icon_` CHAR(72), `IconIndex` SHORT, `ShowCmd` SHORT, `WkDir` CHAR(72)' '`Shortcut`'
    Table 'CreateFolder' '`Directory_` CHAR(72) NOT NULL, `Component_` CHAR(72) NOT NULL' '`Directory_`, `Component_`'
    Table 'Condition' '`Feature_` CHAR(38) NOT NULL, `Level` SHORT NOT NULL, `Condition` CHAR(255)' '`Feature_`, `Level`'
    Table 'LaunchCondition' '`Condition` CHAR(255) NOT NULL, `Description` CHAR(255) NOT NULL LOCALIZABLE' '`Condition`'
    Table 'Upgrade' '`UpgradeCode` CHAR(38) NOT NULL, `VersionMin` CHAR(20), `VersionMax` CHAR(20), `Language` CHAR(255), `Attributes` LONG NOT NULL, `Remove` CHAR(255), `ActionProperty` CHAR(72) NOT NULL' '`UpgradeCode`, `VersionMin`, `VersionMax`, `Language`, `Attributes`'
    foreach($name in @('InstallExecuteSequence','InstallUISequence')){Table $name '`Action` CHAR(72) NOT NULL, `Condition` CHAR(255), `Sequence` SHORT' '`Action`'}
    Table 'Icon' '`Name` CHAR(72) NOT NULL, `Data` OBJECT NOT NULL' '`Name`'
    Table 'Binary' '`Name` CHAR(72) NOT NULL, `Data` OBJECT NOT NULL' '`Name`'
    Table 'CustomAction' '`Action` CHAR(72) NOT NULL, `Type` SHORT NOT NULL, `Source` CHAR(72), `Target` CHAR(255)' '`Action`'
    Table 'Dialog' '`Dialog` CHAR(72) NOT NULL, `HCentering` SHORT NOT NULL, `VCentering` SHORT NOT NULL, `Width` SHORT NOT NULL, `Height` SHORT NOT NULL, `Attributes` LONG, `Title` CHAR(128) LOCALIZABLE, `Control_First` CHAR(50), `Control_Default` CHAR(50), `Control_Cancel` CHAR(50)' '`Dialog`'
    Table 'Control' '`Dialog_` CHAR(72) NOT NULL, `Control` CHAR(50) NOT NULL, `Type` CHAR(20) NOT NULL, `X` SHORT NOT NULL, `Y` SHORT NOT NULL, `Width` SHORT NOT NULL, `Height` SHORT NOT NULL, `Attributes` LONG, `Property` CHAR(72), `Text` CHAR(0) LOCALIZABLE, `Control_Next` CHAR(50), `Help` CHAR(50) LOCALIZABLE' '`Dialog_`, `Control`'
    Table 'ControlEvent' '`Dialog_` CHAR(72) NOT NULL, `Control_` CHAR(50) NOT NULL, `Event` CHAR(50) NOT NULL, `Argument` CHAR(255) NOT NULL, `Condition` CHAR(255) NOT NULL, `Ordering` SHORT' '`Dialog_`, `Control_`, `Event`, `Argument`, `Condition`'
    Table 'EventMapping' '`Dialog_` CHAR(72) NOT NULL, `Control_` CHAR(50) NOT NULL, `Event` CHAR(50) NOT NULL, `Attribute` CHAR(50) NOT NULL' '`Dialog_`, `Control_`, `Event`'
    Table 'CheckBox' '`Property` CHAR(72) NOT NULL, `Value` CHAR(64)' '`Property`'
    Table 'TextStyle' '`TextStyle` CHAR(72) NOT NULL, `FaceName` CHAR(32) NOT NULL, `Size` SHORT NOT NULL, `Color` LONG, `StyleBits` SHORT' '`TextStyle`'
    $product=GuidFor "HITDROL/product/$version/x64"
    $upgrade='{8E86F44E-CF85-4AD0-BFB0-69B2381ACEC1}'
    $properties=@{ProductCode=$product;UpgradeCode=$upgrade;ProductName=$productName;ProductVersion=$version;ProductLanguage='1049';Manufacturer='HITDROL';INSTALLLEVEL='1';DESKTOPSHORTCUT='1';DefaultUIFont='Normal';ARPPRODUCTICON='HitdrolIcon';ARPNOMODIFY='1';SecureCustomProperties='OLDPRODUCTS;NEWERPRODUCTS;AUTOSTART;DESKTOPSHORTCUT;INSTALLDIR';MSIRESTARTMANAGERCONTROL='Disable';REBOOT='ReallySuppress'}
    foreach($key in $properties.Keys){Row Property @($key,$properties[$key])}
    Row Directory @('TARGETDIR',$null,'SourceDir')
    Row Directory @('LocalAppDataFolder','TARGETDIR','.')
    Row Directory @('ProgramsDir','LocalAppDataFolder','Programs')
    Row Directory @('INSTALLDIR','ProgramsDir','HITDROL')
    Row Directory @('ProgramMenuFolder','TARGETDIR','.')
    Row Directory @('AppMenu','ProgramMenuFolder','HITDROL')
    Row Directory @('DesktopFolder','TARGETDIR','.')
    Row Feature @('Main',$null,'HITDROL','Лаунчер и лицензии',1,1,'INSTALLDIR',0)
    Row Feature @('AutoStart',$null,'Автозапуск','Запускать при входе в Windows',2,200,$null,0)
    Row Feature @('Desktop',$null,'Ярлык на рабочем столе','Создать ярлык',3,200,$null,0)
    Row Condition @('AutoStart',1,'AUTOSTART = "1"')
    Row Condition @('Desktop',1,'DESKTOPSHORTCUT = "1"')
    Row LaunchCondition @('VersionNT64','Требуется Windows 11 x64.')
    Row LaunchCondition @('NOT NEWERPRODUCTS','Уже установлена более новая версия HITDROL.')
    Row Upgrade @($upgrade,$null,$version,$null,1,'ALL','OLDPRODUCTS')
    Row Upgrade @($upgrade,$version,$null,$null,2,$null,'NEWERPRODUCTS')
    $directories=@{''='INSTALLDIR'}
    $ddf=@('.Set CabinetNameTemplate=payload.cab','.Set DiskDirectory1=.','.Set Cabinet=ON','.Set Compress=ON','.Set CompressionType=LZX','.Set CompressionMemory=21','.Set MaxDiskSize=0')
    $files=@(Get-ChildItem -LiteralPath $app -Recurse -File|Sort-Object FullName)
    $sequence=0;$mainComponent=$null;$mainFile=$null
    foreach($file in $files){
        $relative=[IO.Path]::GetRelativePath($app,$file.FullName)
        $parent=[IO.Path]::GetDirectoryName($relative)
        if(!$directories.ContainsKey($parent)){
            $chain=[Collections.Generic.List[string]]::new();$part=$parent
            while(!$directories.ContainsKey($part)){$chain.Add($part);$part=[IO.Path]::GetDirectoryName($part)}
            for($i=$chain.Count-1;$i-ge0;$i--){$part=$chain[$i];$key=Id 'D_' $part;$base=[IO.Path]::GetDirectoryName($part);$leaf=[IO.Path]::GetFileName($part);Row Directory @($key,$directories[$base],('D'+$directories.Count.ToString('00000')+'|'+$leaf));$directories[$part]=$key}
        }
        $sequence++;$fid=Id 'F_' $relative;$component=Id 'C_' $relative
        Row Component @($component,(GuidFor "HITDROL/file/$relative"),$directories[$parent],256,$null,$fid)
        Row FeatureComponents @('Main',$component)
        $fileVersion=$null;$language=$null
        if($file.Name-eq'HITDROL.exe'){$fileVersion="$version.0";$language='1049';$mainComponent=$component;$mainFile=$fid}
        Row File @($fid,$component,('F'+$sequence.ToString('00000')+'|'+$file.Name),[int]$file.Length,$fileVersion,$language,512,$sequence)
        Copy-Item -LiteralPath $file.FullName -Destination (Join-Path $cabdir $fid)
        $ddf+='"'+$fid+'" "'+$fid+'"'
    }
    if(!$mainComponent){throw 'Missing HITDROL.exe in payload'}
    foreach($directory in $directories.Values){Row CreateFolder @($directory,$mainComponent)}
    Row CreateFolder @('AppMenu',$mainComponent)
    Row Registry @('InstallLocation',1,'Software\HITDROLLauncher','InstallDir','[INSTALLDIR]',$mainComponent)
    Row Component @('StartupComponent',(GuidFor 'HITDROL/autostart'),'INSTALLDIR',260,$null,'StartupEntry')
    Row FeatureComponents @('AutoStart','StartupComponent')
    Row Registry @('StartupEntry',1,'Software\Microsoft\Windows\CurrentVersion\Run','HITDROL','"[INSTALLDIR]HITDROL.exe" --autostart','StartupComponent')
    Row RemoveRegistry @('RemoveStartup',1,'Software\Microsoft\Windows\CurrentVersion\Run','HITDROL',$mainComponent)
    Row CustomAction @('UninstallStartupRows',1,'RegistryCleanup','CleanupStartup')
    Row CustomAction @('RemoveLegacyStartupTask',1,'RegistryCleanup','RemoveLegacyStartupTask')
    Row Component @('DesktopComponent',(GuidFor 'HITDROL/desktop'),'INSTALLDIR',260,$null,'DesktopMarker')
    Row FeatureComponents @('Desktop','DesktopComponent')
    Row Registry @('DesktopMarker',1,'Software\HITDROLLauncher','DesktopShortcut','#1','DesktopComponent')
    Row Shortcut @('MenuLink','AppMenu',$productName,$mainComponent,'[INSTALLDIR]HITDROL.exe',$null,'HITDROL — ваши игры',$null,'HitdrolIcon',0,1,'INSTALLDIR')
    Row Shortcut @('DesktopLink','DesktopFolder',$productName,'DesktopComponent','[INSTALLDIR]HITDROL.exe',$null,'HITDROL — ваши игры',$null,'HitdrolIcon',0,1,'INSTALLDIR')
    Row TextStyle @('Normal','Segoe UI',9,0,0)
    Row TextStyle @('Title','Segoe UI',20,0,1)
    foreach($property in @('AUTOSTART','DESKTOPSHORTCUT')){Row CheckBox @($property,'1')}
    function Control($Dialog,$Name,$Type,$X,$Y,$Width,$Height,$Property,$Text,$Next,$Attributes=3){Row Control @($Dialog,$Name,$Type,$X,$Y,$Width,$Height,$Attributes,$Property,$Text,$Next,$null)}
    function Event($Dialog,$Control,$Name,$Argument,$Condition,$Order){Row ControlEvent @($Dialog,$Control,$Name,$Argument,$Condition,$Order)}
    Row Dialog @('OptionsDlg',50,50,370,285,3,'[ProductName] — установка','PathLabel','Install','Cancel')
    Control OptionsDlg Title Text 20 18 330 30 $null '{\Title}[ProductName]' $null
    Control OptionsDlg Info Text 20 60 330 35 $null 'Установка для текущего пользователя. Библиотека и настройки сохраняются отдельно от приложения.' $null
    Control OptionsDlg PathLabel Text 20 101 330 15 $null 'Папка установки:' Path
    Control OptionsDlg Path PathEdit 20 120 240 22 INSTALLDIR '{240}' Browse
    Control OptionsDlg Browse PushButton 270 120 80 22 $null 'Обзор…' Startup
    Event OptionsDlg Browse '[HITDROL_BROWSE_PATH]' '[INSTALLDIR]' '1' 1
    Event OptionsDlg Browse SpawnDialog BrowseDlg '1' 2
    Control OptionsDlg Startup CheckBox 20 160 330 18 AUTOSTART 'Запускать HITDROL при входе в Windows' Desktop
    Control OptionsDlg Desktop CheckBox 20 187 330 18 DESKTOPSHORTCUT 'Создать ярлык на рабочем столе' Install
    Control OptionsDlg Install PushButton 180 250 80 22 $null 'Установить' Cancel
    Control OptionsDlg Cancel PushButton 270 250 80 22 $null 'Отмена' PathLabel
    # Checkbox values are chosen AFTER CostFinalize. Feature ControlEvents apply
    # them immediately; setting ADDLOCAL here leaves the previous feature state.
    Event OptionsDlg Install SetTargetPath INSTALLDIR '1' 1
    Event OptionsDlg Install AddLocal Main '1' 2
    Event OptionsDlg Install AddLocal AutoStart 'AUTOSTART = "1"' 3
    Event OptionsDlg Install Remove AutoStart 'NOT AUTOSTART = "1"' 4
    Event OptionsDlg Install AddLocal Desktop 'DESKTOPSHORTCUT = "1"' 5
    Event OptionsDlg Install Remove Desktop 'NOT DESKTOPSHORTCUT = "1"' 6
    Event OptionsDlg Install EndDialog Return '1' 10
    Event OptionsDlg Cancel EndDialog Exit '1' 1
    Row Dialog @('BrowseDlg',50,50,370,300,3,'[ProductName] — выбор папки','FolderLabel','Choose','Cancel')
    Control BrowseDlg FolderLabel Text 20 15 330 16 $null 'Выберите диск и папку для HITDROL:' Disks
    Control BrowseDlg Disks DirectoryCombo 20 38 240 80 HITDROL_BROWSE_PATH $null Up 196611
    Control BrowseDlg Up PushButton 270 38 80 22 $null 'На уровень выше' NewFolder
    Control BrowseDlg NewFolder PushButton 270 68 80 22 $null 'Новая папка' Folders
    Control BrowseDlg Folders DirectoryList 20 70 240 145 HITDROL_BROWSE_PATH 'Папки' FolderPath
    Control BrowseDlg FolderPath PathEdit 20 225 330 22 HITDROL_BROWSE_PATH '{240}' Choose
    Control BrowseDlg Choose PushButton 180 267 80 22 $null 'Выбрать' Cancel
    Control BrowseDlg Cancel PushButton 270 267 80 22 $null 'Отмена' FolderLabel
    Event BrowseDlg Up DirectoryListUp '0' '1' 1
    Event BrowseDlg NewFolder DirectoryListNew '0' '1' 1
    Row EventMapping @('BrowseDlg','Folders','DirectoryListUp','DirectoryListUp')
    Row EventMapping @('BrowseDlg','Folders','DirectoryListNew','DirectoryListNew')
    Event BrowseDlg Choose '[INSTALLDIR]' '[HITDROL_BROWSE_PATH]' '1' 1
    Event BrowseDlg Choose SetTargetPath INSTALLDIR '1' 2
    Event BrowseDlg Choose EndDialog Return '1' 3
    Event BrowseDlg Cancel EndDialog Return '1' 1
    Row Dialog @('MaintenanceDlg',50,50,370,210,3,'[ProductName] — обслуживание','Repair','Repair','Cancel')
    Control MaintenanceDlg Title Text 20 20 330 32 $null '{\Title}[ProductName]' $null
    Control MaintenanceDlg Info Text 20 70 330 50 $null 'Восстановить файлы приложения или удалить HITDROL? Библиотека, избранное и настройки останутся в профиле пользователя.' $null
    Control MaintenanceDlg Repair PushButton 20 170 105 22 $null 'Восстановить' Remove
    Control MaintenanceDlg Remove PushButton 135 170 105 22 $null 'Удалить' Cancel
    Control MaintenanceDlg Cancel PushButton 250 170 100 22 $null 'Отмена' Repair
    Event MaintenanceDlg Repair '[REINSTALL]' ALL '1' 1
    Event MaintenanceDlg Repair '[REINSTALLMODE]' omus '1' 2
    Event MaintenanceDlg Repair EndDialog Return '1' 3
    Event MaintenanceDlg Remove '[REMOVE]' ALL '1' 1
    Event MaintenanceDlg Remove EndDialog Return '1' 2
    Event MaintenanceDlg Cancel EndDialog Exit '1' 1
    Row Dialog @('CompleteDlg',50,50,370,190,3,'[ProductName] — готово','Done','Done','Done')
    Control CompleteDlg Title Text 20 20 330 32 $null '{\Title}Готово' $null
    Control CompleteDlg Info Text 20 70 330 60 $null 'Операция завершена. Библиотека, избранное и пользовательские настройки хранятся отдельно от файлов приложения.' $null
    Control CompleteDlg Done PushButton 260 150 90 22 $null 'Закрыть' Done
    Event CompleteDlg Done EndDialog Return '1' 1
    foreach($row in @(@('FindRelatedProducts',$null,25),@('LaunchConditions',$null,100),@('CostInitialize',$null,800),@('FileCost',$null,900),@('CostFinalize',$null,1000),@('OptionsDlg','NOT Installed',1250),@('MaintenanceDlg','Installed AND NOT REINSTALL AND NOT REMOVE',1260),@('ExecuteAction',$null,1300),@('CompleteDlg',$null,-1))){Row InstallUISequence $row}
    foreach($row in @(@('FindRelatedProducts',$null,25),@('LaunchConditions',$null,100),@('CostInitialize',$null,800),@('FileCost',$null,900),@('CostFinalize',$null,1000),@('MigrateFeatureStates',$null,1200),@('InstallValidate',$null,1400),@('InstallInitialize',$null,1500),@('ProcessComponents',$null,1600),@('UnpublishFeatures',$null,1800),@('RemoveRegistryValues',$null,2600),@('RemoveShortcuts',$null,3200),@('RemoveFiles',$null,3500),@('RemoveFolders',$null,3600),@('CreateFolders',$null,3700),@('InstallFiles',$null,4000),@('CreateShortcuts',$null,4500),@('WriteRegistryValues',$null,5000),@('RegisterUser',$null,6000),@('RegisterProduct',$null,6100),@('PublishFeatures',$null,6300),@('PublishProduct',$null,6400),@('InstallExecute',$null,6500),@('RemoveExistingProducts','OLDPRODUCTS',6600),@('InstallFinalize',$null,6700))){Row InstallExecuteSequence $row}
    Row InstallExecuteSequence @('UninstallStartupRows','REMOVE ~= "ALL"',2500)
    Row InstallExecuteSequence @('RemoveLegacyStartupTask','NOT (REMOVE ~= "ALL")',6750)
    # Remove the older product before writing the new Run entry: its uninstaller
    # must not delete the entry that belongs to the newly installed executable.
    Exec 'UPDATE `InstallExecuteSequence` SET `Sequence` = 1510 WHERE `Action` = ''RemoveExistingProducts'''
    Exec 'DELETE FROM `InstallExecuteSequence` WHERE `Action` = ''MigrateFeatureStates'''
    [IO.File]::WriteAllLines((Join-Path $cabdir 'payload.ddf'),$ddf,[Text.Encoding]::ASCII)
    $info=[Diagnostics.ProcessStartInfo]::new((Get-Command makecab.exe).Source);$info.WorkingDirectory=$cabdir;$info.UseShellExecute=$false;$info.ArgumentList.Add('/F');$info.ArgumentList.Add('payload.ddf');$info.RedirectStandardOutput=$true
    $process=[Diagnostics.Process]::Start($info);$output=$process.StandardOutput.ReadToEnd();$process.WaitForExit();if($process.ExitCode){throw "Cabinet failed: $output"}
    Row Media @(1,$sequence,$null,'#payload.cab',$null,$null)
    $view=$db.OpenView('INSERT INTO `_Streams` (`Name`,`Data`) VALUES (?,?)');$record=$installer.CreateRecord(2);$record.StringData(1)='payload.cab';$record.SetStream(2,(Join-Path $cabdir 'payload.cab'));$view.Execute($record);$view.Close()
    $icon=Get-ChildItem -Path (Join-Path $repo 'target/release/build/hitdrol-launcher-*/out/hitdrol.ico')|Sort-Object LastWriteTime -Descending|Select-Object -First 1
    if(!$icon){throw 'Installer icon missing'}
    $view=$db.OpenView('INSERT INTO `Icon` (`Name`,`Data`) VALUES (?,?)');$record=$installer.CreateRecord(2);$record.StringData(1)='HitdrolIcon';$record.SetStream(2,$icon.FullName);$view.Execute($record);$view.Close()
    $view=$db.OpenView('INSERT INTO `Binary` (`Name`,`Data`) VALUES (?,?)');$record=$installer.CreateRecord(2);$record.StringData(1)='RegistryCleanup';$record.SetStream(2,(Join-Path $repo 'target/installer-actions/release/hitdrol_installer_actions.dll'));$view.Execute($record);$view.Close()
    $summary=$db.SummaryInformation(20)
    $summary.Property(1)=1251;$summary.Property(2)=$productName;$summary.Property(3)='Установка '+$productName;$summary.Property(4)='HITDROL';$summary.Property(7)='x64;1049';$summary.Property(9)=[guid]::NewGuid().ToString('B').ToUpperInvariant();$summary.Property(14)=500;$summary.Property(15)=10;$summary.Property(18)='HITDROL native MSI builder';$summary.Persist()
    $db.Commit()
    foreach($object in @($summary,$view,$record,$db)){[Runtime.InteropServices.Marshal]::FinalReleaseComObject($object)|Out-Null}
    $summary=$null;$view=$null;$record=$null;$db=$null
    [GC]::Collect();[GC]::WaitForPendingFinalizers();[GC]::Collect()
    & (Join-Path $PSScriptRoot 'verify-installer-dialogs.ps1') -Package $msi
    $hash=(Get-FileHash -LiteralPath $msi -Algorithm SHA256).Hash.ToLowerInvariant()
    "$hash  $([IO.Path]::GetFileName($msi))"|Set-Content -LiteralPath ($msi+'.sha256') -Encoding ascii
    Write-Output "Installer: $msi"
    Write-Output "ProductCode: $product"
    Write-Output "SHA256: $hash"
}finally{
    if($stage -and (Test-Path -LiteralPath $stage)){
        $resolved=(Resolve-Path -LiteralPath $stage).Path
        $allowed=[IO.Path]::GetFullPath((Join-Path $repo 'artifacts'))+[IO.Path]::DirectorySeparatorChar
        if(!$resolved.StartsWith($allowed,[StringComparison]::OrdinalIgnoreCase) -or (Get-Item -LiteralPath $resolved).Attributes -band [IO.FileAttributes]::ReparsePoint){throw 'Unsafe installer staging path'}
        Remove-Item -LiteralPath $resolved -Recurse -Force
    }
    Pop-Location
}
