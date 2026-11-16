param([string]$OutputPath)
$ErrorActionPreference = 'Stop'
$projectRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
if (-not $OutputPath) { $OutputPath = Join-Path $projectRoot 'THIRD_PARTY_LICENSES.txt' }
Push-Location $projectRoot
try {
    $metadataText = & cargo metadata --locked --format-version 1 --features desktop --filter-platform x86_64-pc-windows-msvc
    if ($LASTEXITCODE -ne 0) { throw 'Could not resolve dependency license metadata' }
    $metadata = $metadataText | ConvertFrom-Json
    $resolved = [Collections.Generic.HashSet[string]]::new()
    $supplemental = Get-Content -LiteralPath (Join-Path $projectRoot 'licenses/upstream-notices.json') -Raw | ConvertFrom-Json
    foreach ($node in $metadata.resolve.nodes) { [void]$resolved.Add($node.id) }
    $notices = [Text.StringBuilder]::new()
    [void]$notices.AppendLine('Roynit Music third-party Rust dependency notices')
    [void]$notices.AppendLine('Generated from Cargo.lock and resolved registry package license files.')
    foreach ($package in ($metadata.packages | Where-Object { $_.source -and $resolved.Contains($_.id) } | Sort-Object name,version)) {
        [void]$notices.AppendLine("`n============================================================")
        [void]$notices.AppendLine(($package.name + ' ' + $package.version + ' | ' + $package.license))
        if ($package.repository) { [void]$notices.AppendLine($package.repository) }
        $packageRoot = Split-Path $package.manifest_path
        $files = @(Get-ChildItem -LiteralPath $packageRoot -File -Recurse | Where-Object { $_.Name -match '^(LICENSE|LICENCE|COPYING|COPYRIGHT|NOTICE|OFL|UFL)([._-].*)?$' -and $_.Length -lt 1048576 } | Sort-Object FullName)
        if ($package.license_file) {
            $explicit = Join-Path $packageRoot $package.license_file
            if (Test-Path -LiteralPath $explicit -PathType Leaf) { $files += Get-Item -LiteralPath $explicit }
        }
        if ($files.Count -eq 0) {
            $key = $package.name + '@' + $package.version
            $extra = $supplemental.PSObject.Properties[$key].Value
            if (-not $extra) { throw ('Missing upstream license text for ' + $key) }
            foreach ($notice in $extra) {
                [void]$notices.AppendLine(('--- Upstream: ' + $notice.url))
                [void]$notices.AppendLine($notice.text)
            }
        }
        foreach ($file in ($files | Sort-Object FullName -Unique)) {
            $prefix = $packageRoot.TrimEnd([char[]]'\/') + [IO.Path]::DirectorySeparatorChar
            $label = if ($file.FullName.StartsWith($prefix,[StringComparison]::OrdinalIgnoreCase)) { $file.FullName.Substring($prefix.Length) } else { $file.Name }
            [void]$notices.AppendLine(('--- ' + $label))
            [void]$notices.AppendLine([IO.File]::ReadAllText($file.FullName))
        }
    }
    [IO.File]::WriteAllText([IO.Path]::GetFullPath($OutputPath),$notices.ToString(),[Text.UTF8Encoding]::new($false))
    Write-Output $OutputPath
} finally { Pop-Location }
