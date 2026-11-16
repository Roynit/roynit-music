# Publisher signing

Code signing requires a real publisher certificate with a private key and Code Signing EKU. No certificate is included. These scripts do not create a self-signed certificate, import a private key or modify trusted roots.

For an authorized certificate already in Windows CurrentUser/My, run:

```powershell
./scripts/sign.ps1 -Thumbprint YOUR_CERTIFICATE_THUMBPRINT -Files @('target/release/riff-desktop.exe','target/release/riff.exe')
./scripts/package.ps1 -SkipBuild -RequireSigned
```

The script uses Windows SDK SignTool, SHA-256, an RFC3161 timestamp and Windows signature verification. Hardware-backed certificates can use their installed Windows key provider; provider authentication is performed by the certificate owner. Cloud signing can run externally before packaging with RequireSigned. Account/payment/identity setup is not automated here.

Do not place private keys, certificate passwords or PFX files in source control or chat. Live signing must be validated once an authorized publisher certificate is available.

Without a certificate, signing-status.json records NotSigned. The hosted release workflow currently builds this unsigned variant. SHA-256 proves consistency with the release manifest; it is not a trusted publisher signature or a guarantee against SmartScreen reputation prompts.
