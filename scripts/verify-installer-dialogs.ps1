param([Parameter(Mandatory)][string]$Package)
$ErrorActionPreference='Stop'
$Package=(Resolve-Path -LiteralPath $Package).Path
# Validate the authored dialog graph, including Control_First. Quiet installation
# and feature-state checks do not construct these dialogs (MSI error 2834).
$installer=New-Object -ComObject WindowsInstaller.Installer
$db=$installer.OpenDatabase($Package,0)
$view=$null;$record=$null
try {
    $dialogs=@{}
    $view=$db.OpenView('SELECT `Dialog`,`Control_First`,`Control_Default`,`Control_Cancel`,`Width`,`Height` FROM `Dialog`')
    $view.Execute()
    while($record=$view.Fetch()){
        $dialogs[$record.StringData(1)]=@{First=$record.StringData(2);Default=$record.StringData(3);Cancel=$record.StringData(4);Width=$record.IntegerData(5);Height=$record.IntegerData(6);Controls=@{}}
        [void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($record);$record=$null
    }
    $view.Close();[void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($view)
    $view=$db.OpenView('SELECT `Dialog_`,`Control`,`Type`,`Control_Next`,`Property`,`X`,`Y`,`Width`,`Height` FROM `Control`')
    $view.Execute()
    while($record=$view.Fetch()){
        $dialog=$record.StringData(1);$name=$record.StringData(2)
        if(!$dialogs.ContainsKey($dialog)){throw "Unknown dialog for $dialog.$name"}
        $dialogs[$dialog].Controls[$name]=@{Type=$record.StringData(3);Next=$record.StringData(4);Property=$record.StringData(5);X=$record.IntegerData(6);Y=$record.IntegerData(7);Width=$record.IntegerData(8);Height=$record.IntegerData(9)}
        [void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($record);$record=$null
    }
    $view.Close()
    foreach($name in ($dialogs.Keys|Sort-Object)){
        $dialog=$dialogs[$name];$controls=$dialog.Controls
        foreach($entry in @($dialog.First,$dialog.Default,$dialog.Cancel)){
            if(!$entry -or !$controls.ContainsKey($entry)){throw "Invalid initial/default/cancel control: $name.$entry"}
        }
        foreach($id in $controls.Keys){
            $control=$controls[$id]
            if($control.Next -and !$controls.ContainsKey($control.Next)){throw "Unknown next control: $name.$id -> $($control.Next)"}
            if($control.Type-in@('CheckBox','Edit','PathEdit','DirectoryCombo','DirectoryList') -and !$control.Property){throw "Missing property for $name.$id"}
            if($control.X-lt0 -or $control.Y-lt0 -or ($control.X+$control.Width)-gt$dialog.Width -or ($control.Y+$control.Height)-gt$dialog.Height){throw "Control outside dialog bounds: $name.$id"}
        }
        $visited=[Collections.Generic.HashSet[string]]::new()
        $current=$dialog.First
        while($current -and $visited.Add($current)){$current=$controls[$current].Next}
        if($current-ne$dialog.First){throw "MSI 2834: $name does not close its tab loop at Control_First ($($dialog.First))"}
        foreach($id in $controls.Keys){
            $control=$controls[$id]
            if(($control.Next -or $control.Type-in@('PushButton','CheckBox','Edit','PathEdit','DirectoryCombo','DirectoryList')) -and !$visited.Contains($id)){throw "MSI 2834: $name.$id is outside the dialog's single tab loop"}
        }
        [pscustomobject]@{Dialog=$name;First=$dialog.First;TabControls=$visited.Count;SingleLoop=$true}
    }
} finally {
    foreach($object in @($record,$view,$db,$installer)){if($object){[void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($object)}}
}
