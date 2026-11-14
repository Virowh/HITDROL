param([switch]$Apply)
$ErrorActionPreference='Stop'
$repo=[IO.Path]::GetFullPath((Split-Path $PSScriptRoot -Parent))
$version=(Get-Content -LiteralPath (Join-Path $repo 'VERSION') -Raw).Trim()
$dist=Join-Path $repo 'dist'
$keep=@("HITDROL-$($version.Replace(' ','-'))-windows-x64.msi","HITDROL-$($version.Replace(' ','-'))-windows-x64.msi.sha256","HITDROL-$($version.Replace(' ','-'))-windows-x64.zip","HITDROL-$($version.Replace(' ','-'))-windows-x64.zip.sha256")
foreach($extension in @('msi','zip')){
    $package=Join-Path $dist "HITDROL-$($version.Replace(' ','-'))-windows-x64.$extension"
    $expected=((Get-Content -LiteralPath ($package+'.sha256') -Raw)-split '\s+')[0]
    if((Get-FileHash -LiteralPath $package).Hash -ine $expected){throw "Missing or invalid current $extension package; cleanup refused"}
}
$targets=@(Join-Path $repo 'target';Join-Path $repo 'artifacts';Join-Path $repo 'installer-actions/target')
$targets+=@(Get-ChildItem -LiteralPath $dist -Force|Where-Object {$_.Name-notin$keep}|Select-Object -ExpandProperty FullName)
$removed=0L
foreach($target in $targets){
    if(!(Test-Path -LiteralPath $target)){continue}
    $resolved=(Resolve-Path -LiteralPath $target).Path
    if(!$resolved.StartsWith($repo+[IO.Path]::DirectorySeparatorChar,[StringComparison]::OrdinalIgnoreCase)){throw "Cleanup escaped workspace: $resolved"}
    $item=Get-Item -LiteralPath $resolved -Force
    if($item.Attributes-band[IO.FileAttributes]::ReparsePoint){throw "Refusing linked cleanup root: $resolved"}
    $bytes=if($item.PSIsContainer){(Get-ChildItem -LiteralPath $resolved -Recurse -File -Force|Measure-Object Length -Sum).Sum}else{$item.Length}
    [pscustomobject]@{Path=[IO.Path]::GetRelativePath($repo,$resolved);MiB=[math]::Round($bytes/1MB,1);Deleted=[bool]$Apply}
    if($Apply){Remove-Item -LiteralPath $resolved -Recurse -Force;$removed+=$bytes}
}
if($Apply){Write-Output "Removed approximately $([math]::Round($removed/1GB,2)) GiB. Source and current MSI/ZIP retained."}
