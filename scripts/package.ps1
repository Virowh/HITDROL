param([switch]$SkipChecks)
$ErrorActionPreference = 'Stop'
$repo = Split-Path $PSScriptRoot -Parent
Push-Location $repo
try {
    if (!$SkipChecks) {
        cargo test --all-targets --locked
        if ($LASTEXITCODE) { throw 'Tests failed' }
        cargo clippy --all-targets --locked -- -D warnings
        if ($LASTEXITCODE) { throw 'Clippy failed' }
    }
    cargo build --release --locked --bin hitdrol-launcher
    if ($LASTEXITCODE) { throw 'Build failed' }
    $metadata = (cargo metadata --locked --format-version 1 --filter-platform x86_64-pc-windows-msvc | ConvertFrom-Json)
    if ($LASTEXITCODE) { throw 'Dependency metadata failed' }
    $version = ($metadata.packages | Where-Object name -EQ 'hitdrol-launcher').version
    $release = (Get-Content -LiteralPath (Join-Path $repo 'VERSION') -Raw).Trim().Replace(' ','-')
    $name = "HITDROL-$release-windows-x64"
    $dist = Join-Path $repo 'dist'
    # Use a fresh staging folder and remove it after packaging.
    $stage = Join-Path $dist ("staging-" + [guid]::NewGuid().ToString('N'))
    $folder = Join-Path $stage $name
    $licenses = Join-Path $folder 'licenses'
    New-Item -ItemType Directory -Force -Path $licenses | Out-Null
    Copy-Item -LiteralPath (Join-Path $repo 'target/release/hitdrol-launcher.exe') -Destination (Join-Path $folder 'HITDROL.exe')
    foreach ($file in @('README.md','LICENSE','QA.md')) { Copy-Item -LiteralPath (Join-Path $repo $file) -Destination $folder }
    Copy-Item -LiteralPath (Join-Path $repo 'assets/hitdrol-logo.png') -Destination (Join-Path $folder 'HITDROL-logo.png')
    $ids = [Collections.Generic.HashSet[string]]::new()
    foreach ($node in $metadata.resolve.nodes) { [void]$ids.Add($node.id) }
    $notices = @('HITDROL third-party dependencies', 'License files are included in licenses/<crate>-<version>/', '')
    foreach ($package in ($metadata.packages | Sort-Object name,version)) {
        if (!$ids.Contains($package.id) -or $package.name -eq 'hitdrol-launcher') { continue }
        $notices += "$($package.name) $($package.version) | $($package.license) | $($package.repository)"
        $source = Split-Path $package.manifest_path -Parent
        $destination = Join-Path $licenses "$($package.name)-$($package.version)"
        $files = @(Get-ChildItem -LiteralPath $source -File -Recurse | Where-Object { $_.Name -match '(?i)(licen[sc]e|copyright|copying|notice)|^(OFL|UFL)([.-]|$)|^Hack-Regular.txt$' })
        if ($package.license_file) { $files += Get-Item -LiteralPath (Join-Path $source $package.license_file) }
        foreach ($file in ($files | Sort-Object FullName -Unique)) {
            $relative = [IO.Path]::GetRelativePath($source, $file.FullName)
            $target = Join-Path $destination $relative
            New-Item -ItemType Directory -Force -Path (Split-Path $target -Parent) | Out-Null
            Copy-Item -LiteralPath $file.FullName -Destination $target
        }
    }
    $notices | Set-Content -LiteralPath (Join-Path $folder 'THIRD_PARTY_NOTICES.txt') -Encoding utf8
    Get-ChildItem -LiteralPath $folder -File -Recurse | Where-Object LastWriteTime -LT ([datetime]'1980-01-01') | ForEach-Object { $_.LastWriteTime = [datetime]'1980-01-01' }
    $zip = Join-Path $dist ($name + '.zip')
    Compress-Archive -LiteralPath $folder -DestinationPath $zip -Force
    $hash = (Get-FileHash -LiteralPath $zip -Algorithm SHA256).Hash.ToLowerInvariant()
    "$hash  $name.zip" | Set-Content -LiteralPath ($zip + '.sha256') -Encoding ascii
    Write-Output "Package: $zip"
    Write-Output "SHA256: $hash"
} finally {
    if($stage -and (Test-Path -LiteralPath $stage)){
        $resolved=(Resolve-Path -LiteralPath $stage).Path
        $allowed=[IO.Path]::GetFullPath((Join-Path $repo 'dist'))+[IO.Path]::DirectorySeparatorChar
        if(!$resolved.StartsWith($allowed,[StringComparison]::OrdinalIgnoreCase) -or (Get-Item -LiteralPath $resolved).Attributes -band [IO.FileAttributes]::ReparsePoint){throw 'Unsafe package staging path'}
        Remove-Item -LiteralPath $resolved -Recurse -Force
    }
    Pop-Location
}
