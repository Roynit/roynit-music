param([string]$Thumbprint = $env:WINDOWS_SIGN_THUMBPRINT,[string]$TimestampUrl = 'https://timestamp.digicert.com',[string[]]$Files)
$ErrorActionPreference = 'Stop'
if (-not $Thumbprint -or $Thumbprint -notmatch '^[0-9a-fA-F]{40}$') { throw 'A real code-signing certificate thumbprint is required. No certificate was configured.' }
$certificate = Get-Item -LiteralPath ('Cert:\CurrentUser\My\' + $Thumbprint) -ErrorAction Stop
if (-not $certificate.HasPrivateKey -or $certificate.NotAfter -le (Get-Date)) { throw 'The signing certificate has no usable private key or has expired' }
if (-not ($certificate.EnhancedKeyUsageList.ObjectId -contains '1.3.6.1.5.5.7.3.3')) { throw 'Certificate is not valid for code signing' }
if (-not $Files) { throw 'Specify the release executables to sign' }
$tool = Get-Command signtool.exe -ErrorAction SilentlyContinue
if ($tool) { $signToolPath = $tool.Source } else {
    $kit = Join-Path ${env:ProgramFiles(x86)} 'Windows Kits\10\bin'
    $signToolPath = Get-ChildItem -LiteralPath $kit -Filter signtool.exe -File -Recurse -ErrorAction SilentlyContinue | Where-Object { $_.Directory.Name -eq 'x64' } | Sort-Object FullName -Descending | Select-Object -First 1 -ExpandProperty FullName
}
if (-not $signToolPath) { throw 'Windows SDK SignTool is required for signing' }
foreach ($file in $Files) {
    $resolved = (Resolve-Path -LiteralPath $file).Path
    & $signToolPath sign /sha1 $Thumbprint /fd SHA256 /tr $TimestampUrl /td SHA256 /d 'Roynit Music' $resolved
    if ($LASTEXITCODE -ne 0) { throw 'Code signing failed' }
    & $signToolPath verify /pa /all $resolved
    if ($LASTEXITCODE -ne 0) { throw 'Windows signature verification failed' }
    $signature = Get-AuthenticodeSignature -LiteralPath $resolved
    if ($signature.Status -ne 'Valid' -or $signature.SignerCertificate.Thumbprint -ne $Thumbprint -or -not $signature.TimeStamperCertificate) { throw 'Signature, signer or timestamp validation failed' }
}
