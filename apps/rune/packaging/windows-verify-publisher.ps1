param([Parameter(Mandatory=$true)][string]$Package, [Parameter(Mandatory=$true)][string]$Publisher)
$ErrorActionPreference = 'Stop'
$signature = Get-AuthenticodeSignature -LiteralPath $Package
if ($signature.Status -ne 'Valid') { throw "Invalid signature: $($signature.Status)" }
$expected = [System.Security.Cryptography.X509Certificates.X500DistinguishedName]::new($Publisher)
if ($signature.SignerCertificate.SubjectName.Name -ne $expected.Name) { throw 'Certificate subject does not match MSIX Publisher' }
