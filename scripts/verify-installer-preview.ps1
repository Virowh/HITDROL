param([string]$Package)
$ErrorActionPreference='Stop'
$repo=Split-Path $PSScriptRoot -Parent
$release=(Get-Content -LiteralPath (Join-Path $repo 'VERSION') -Raw).Trim().Replace(' ','-')
if(!$Package){$Package=Join-Path $repo "dist/HITDROL-$release-windows-x64.msi"}
$Package=(Resolve-Path -LiteralPath $Package).Path
$stage=Join-Path $repo ('artifacts/dialog-preview-'+[guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $stage -Force|Out-Null
$copy=Join-Path $stage 'preview-only.msi'
$database=0u;$preview=0u;$previousUi=0u
# Native authoring preview constructs actual modeless, inactive dialogs. There
# is no installation, screenshot capture, input injection or msiexec automation.
Add-Type -TypeDefinition @'
using System;using System.Runtime.InteropServices;
public static class HitdrolDialogPreview {
 [DllImport("msi.dll",CharSet=CharSet.Unicode,ExactSpelling=true)] public static extern uint MsiOpenDatabaseW(string path,IntPtr mode,out uint database);
 [DllImport("msi.dll")] public static extern uint MsiEnableUIPreview(uint database,out uint preview);
 [DllImport("msi.dll",CharSet=CharSet.Unicode,ExactSpelling=true)] public static extern uint MsiPreviewDialogW(uint preview,string name);
 [DllImport("msi.dll")] public static extern uint MsiCloseHandle(uint handle);
 [DllImport("msi.dll")] public static extern uint MsiSetInternalUI(uint level,IntPtr parent);
}
'@
try {
    Copy-Item -LiteralPath $Package -Destination $copy
    $installer=New-Object -ComObject WindowsInstaller.Installer
    $db=$installer.OpenDatabase($copy,1)
    # Preview has no costing session. Seed valid paths in this disposable copy.
    foreach($property in @('INSTALLDIR','HITDROL_BROWSE_PATH')){
        $view=$db.OpenView('DELETE FROM `Property` WHERE `Property` = ?');$record=$installer.CreateRecord(1);$record.StringData(1)=$property;$view.Execute($record);$view.Close()
        [void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($record);[void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($view)
        $view=$db.OpenView('INSERT INTO `Property` (`Property`,`Value`) VALUES (?,?)');$record=$installer.CreateRecord(2);$record.StringData(1)=$property;$record.StringData(2)='C:\HITDROL';$view.Execute($record);$view.Close()
        [void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($record);[void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($view)
    }
    $db.Commit()
    [void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($db);[void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($installer)
    $db=$null;$installer=$null;$record=$null;$view=$null
    $previousUi=[HitdrolDialogPreview]::MsiSetInternalUI(2,[IntPtr]::Zero)
    $status=[HitdrolDialogPreview]::MsiOpenDatabaseW($copy,[IntPtr]::Zero,[ref]$database)
    if($status){throw "Preview database open failed: $status"}
    $status=[HitdrolDialogPreview]::MsiEnableUIPreview($database,[ref]$preview)
    if($status){throw "Preview initialization failed: $status"}
    foreach($dialog in @('OptionsDlg','BrowseDlg','MaintenanceDlg','CompleteDlg')){
        $status=[HitdrolDialogPreview]::MsiPreviewDialogW($preview,$dialog)
        if($status){throw "Native dialog creation failed: $dialog (status $status)"}
        [pscustomobject]@{Dialog=$dialog;NativeCreationStatus=$status;Created=$true}
        [void][HitdrolDialogPreview]::MsiPreviewDialogW($preview,$null)
    }
} finally {
    if($preview){[void][HitdrolDialogPreview]::MsiPreviewDialogW($preview,$null);[void][HitdrolDialogPreview]::MsiCloseHandle($preview)}
    if($database){[void][HitdrolDialogPreview]::MsiCloseHandle($database)}
    if($previousUi){[void][HitdrolDialogPreview]::MsiSetInternalUI($previousUi,[IntPtr]::Zero)}
    foreach($object in @($record,$view,$db,$installer)){if($object){[void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($object)}}
    $resolved=(Resolve-Path -LiteralPath $stage).Path
    $allowed=[IO.Path]::GetFullPath((Join-Path $repo 'artifacts'))+[IO.Path]::DirectorySeparatorChar
    if(!$resolved.StartsWith($allowed,[StringComparison]::OrdinalIgnoreCase) -or (Get-Item -LiteralPath $resolved).Attributes-band[IO.FileAttributes]::ReparsePoint){throw 'Unsafe preview staging path'}
    Remove-Item -LiteralPath $resolved -Recurse -Force
}
