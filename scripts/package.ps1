param([switch]$SkipBuild,[switch]$RequireSigned)
$ErrorActionPreference = 'Stop'
$projectRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
Push-Location $projectRoot
try {
    & (Join-Path $PSScriptRoot 'generate-notices.ps1') | Out-Null
    if (-not $SkipBuild) {
        & cargo build --release --locked --features desktop --bins
        if ($LASTEXITCODE -ne 0) { throw 'Roynit build failed' }
    }
    $targetRoot = if ($env:CARGO_TARGET_DIR) { [IO.Path]::GetFullPath($env:CARGO_TARGET_DIR) } else { Join-Path $projectRoot 'target' }
    $releaseRoot = Join-Path $targetRoot 'release'
    $cargoText = Get-Content -LiteralPath (Join-Path $projectRoot 'Cargo.toml') -Raw
    if ($cargoText -notmatch '(?m)^version = "([0-9]+\.[0-9]+\.[0-9]+)"') { throw 'Could not determine release version' }
    $version = $Matches[1]
    $embedded = & (Join-Path $releaseRoot 'riff.exe') --license-info | ConvertFrom-Json
    if ($LASTEXITCODE -ne 0 -or $embedded.notices_sha256 -ne (Get-FileHash -LiteralPath (Join-Path $projectRoot 'THIRD_PARTY_LICENSES.txt') -Algorithm SHA256).Hash.ToLowerInvariant()) { throw 'Embedded dependency notices are stale; rebuild the release before packaging' }
    $distRoot = Join-Path $projectRoot 'dist'
    $bundleRoot = Join-Path $distRoot ('Roy-Music-' + $version + '-windows-x64')
    New-Item -ItemType Directory -Path $bundleRoot -Force | Out-Null
    $signatures = foreach ($name in @('riff-desktop.exe','riff.exe')) {
        $binary = Join-Path $releaseRoot $name
        $signature = Get-AuthenticodeSignature -LiteralPath $binary
        if ($RequireSigned -and $signature.Status -ne 'Valid') { throw ($name + ' has no valid trusted signature') }
        Copy-Item -LiteralPath $binary -Destination (Join-Path $bundleRoot $name) -Force
        [ordered]@{file=$name;status=$signature.Status.ToString();signed=($signature.Status -eq 'Valid')}
    }
    $signatureJson = $signatures | ConvertTo-Json
    [IO.File]::WriteAllText((Join-Path $bundleRoot 'signing-status.json'),$signatureJson,[Text.UTF8Encoding]::new($false))
    foreach ($name in @('LICENSE','THIRD_PARTY_NOTICES.md','THIRD_PARTY_LICENSES.txt')) { Copy-Item -LiteralPath (Join-Path $projectRoot $name) -Destination (Join-Path $bundleRoot $name) -Force }
    [IO.File]::WriteAllText((Join-Path $bundleRoot 'Install Roy Music.cmd'),"@echo off`r`nstart `"`" `"%~dp0riff-desktop.exe`" --install --launch`r`n")
    $readme = "Roy Music by dang — native Rust app for Windows x64.`r`nRecommended: run Roy-Music-$version-Setup.exe. It checks your PC and installs what is missing (FFmpeg, yt-dlp) with verified downloads.`r`nThis archive is the portable copy: run riff-desktop.exe, or Install Roy Music.cmd for a plain per-user install.`r`nExisting library/account data stays separate from application files.`r`nIf FFmpeg is missing, Roy Music offers to install it, or use Settings → Playback → Media engine.`r`nUpdates: https://roynit.dangz.org/?app=roynit-music`r`nSee signing-status.json for actual Windows signature status.`r`nA checksum is not a code-signing certificate.`r`nNo old Electron source/runtime is included. Audio Studio is absent.`r`nRequired upstream and dependency license notices are included.`r`n"
    [IO.File]::WriteAllText((Join-Path $bundleRoot 'README.txt'),$readme,[Text.UTF8Encoding]::new($false))
    $archive = $bundleRoot + '.zip'
    Compress-Archive -Path (Join-Path $bundleRoot '*') -DestinationPath $archive -Force
    $asset = Join-Path $distRoot 'riff-rust-windows-x64.exe'
    Copy-Item -LiteralPath (Join-Path $releaseRoot 'riff-desktop.exe') -Destination $asset -Force
    # The Setup is roy-setup.exe with the app appended (it unpacks itself); the plain app exe above stays
    # as the legacy update asset. Start-Process -Wait because roy-setup is a GUI-subsystem program.
    $setup = Join-Path $distRoot ('Roy-Music-' + $version + '-Setup.exe')
    $payload = @('riff-desktop.exe','riff.exe') | ForEach-Object { Join-Path $releaseRoot $_ }
    $payload += Join-Path $bundleRoot 'signing-status.json'
    $pack = Start-Process -FilePath (Join-Path $releaseRoot 'roy-setup.exe') -ArgumentList (@('--pack',('"' + $setup + '"')) + ($payload | ForEach-Object { '"' + $_ + '"' })) -Wait -PassThru
    if ($pack.ExitCode -ne 0 -or -not (Test-Path -LiteralPath $setup)) { throw 'Could not build the Setup installer' }
    $setupDigest = (Get-FileHash -LiteralPath $setup -Algorithm SHA256).Hash.ToLowerInvariant()
    [IO.File]::WriteAllText(($setup + '.sha256'),$setupDigest + '  ' + (Split-Path -Leaf $setup) + "`n")
    $digest = (Get-FileHash -LiteralPath $asset -Algorithm SHA256).Hash.ToLowerInvariant()
    [IO.File]::WriteAllText(($asset + '.sha256'),$digest + '  riff-rust-windows-x64.exe' + "`n")
    [IO.File]::WriteAllText((Join-Path $distRoot 'signing-status.json'),$signatureJson,[Text.UTF8Encoding]::new($false))
    Write-Output $archive
} finally { Pop-Location }
