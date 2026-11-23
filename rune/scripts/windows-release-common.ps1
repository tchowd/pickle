# Public receipt validation and existing-certificate signing. A receipt is an integrity
# record, not proof of source provenance: the release workflow must authenticate its digest.
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'windows-sdk.ps1')

function Get-ReleaseSha256([string]$Path) { return (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToLowerInvariant() }
function Write-ReleaseJson($Value,[string]$Path) { [IO.File]::WriteAllText($Path,($Value | ConvertTo-Json -Depth 12),[Text.UTF8Encoding]::new($false)) }
function Assert-ReleaseAncestors([string]$Path,[switch]$MayBeMissing) {
  $full=[IO.Path]::GetFullPath($Path);$current=$full;$leaf=$true
  while ($current) {
    if (Test-Path -LiteralPath $current) {
      $item=Get-Item -LiteralPath $current -Force
      if (($item.Attributes -band [IO.FileAttributes]::ReparsePoint) -or (!$leaf -and !$item.PSIsContainer)) { throw 'Release paths must not contain reparse/junction ancestors or non-directory parents.' }
    } elseif (!$MayBeMissing) { throw 'Required release path is missing.' }
    $parent=[IO.Directory]::GetParent($current)
    $current=if ($parent) { $parent.FullName } else { $null }
    $leaf=$false
  }
  return $full
}
function Assert-ReleaseArtifactSize([long]$Size) { if ($Size -le 0 -or $Size -gt 512MB) { throw 'Final installer/archive size is outside the 1-byte to 512-MiB release budget.' } }
function Publish-ReleaseFiles([string]$Work,[string]$Destination,[string[]]$Names,[scriptblock]$MoveOperation) {
  Assert-ReleaseAncestors $Work | Out-Null
  Assert-ReleaseAncestors $Destination | Out-Null
  $prefix=[IO.Path]::GetFullPath($Destination).TrimEnd('\','/')+[IO.Path]::DirectorySeparatorChar
  if (![IO.Path]::GetFullPath($Work).StartsWith($prefix,[StringComparison]::OrdinalIgnoreCase)) { throw 'Promotion work must be owned beneath the fresh destination.' }
  $attempted=@()
  try {
    foreach ($name in $Names) {
      if ($name -notmatch '^[A-Za-z0-9._-]+$' -or $name -in @('.','..')) { throw 'Unsafe release promotion filename.' }
      $source=Assert-ReleasePath $Work $name
      $target=Join-Path $Destination $name
      if (Test-Path -LiteralPath $target) { throw 'Release promotion refuses to overwrite an existing file.' }
      $attempted+=$target
      if ($MoveOperation) { & $MoveOperation $source $target } else { [IO.File]::Move($source,$target) }
    }
  } catch {
    $failure=$_;$rollbackErrors=@()
    foreach ($path in $attempted) {
      try { if (Test-Path -LiteralPath $path) { Remove-Item -LiteralPath $path -Force -ErrorAction Stop } }
      catch { $rollbackErrors+=$path }
    }
    if ($rollbackErrors.Count) { throw ('Release promotion failed; rollback could not remove: '+($rollbackErrors -join ', ')) }
    throw $failure
  } finally {
    if (Test-Path -LiteralPath $Work) { Remove-Item -LiteralPath $Work -Recurse -Force -ErrorAction Stop }
  }
}
function Assert-ReleaseIdentity([string]$Name,[string]$Publisher,[string]$Version) {
  if ($Name -notmatch '^[A-Za-z0-9][A-Za-z0-9.-]{1,48}[A-Za-z0-9]$' -or $Publisher -notmatch '^CN=' -or $Publisher.Contains([char]0)) { throw 'An explicit valid package name and exact publisher subject are required.' }
  if ($Version -notmatch '^\d+\.\d+\.\d+\.\d+$' -or @($Version.Split('.') | Where-Object { [uint64]$_ -gt 65535 }).Count) { throw 'Supply a four-part MSIX version, each component from 0 to 65535.' }
}
function Assert-ReleasePath([string]$Root,[string]$Relative) {
  if (!$Relative -or $Relative.Contains('\') -or [IO.Path]::IsPathRooted($Relative) -or @($Relative.Split('/') | Where-Object { !$_ -or $_ -eq '.' -or $_ -eq '..' -or $_.Contains(':') }).Count) { throw 'Unsafe prepared payload path.' }
  $current=$Root
  foreach ($part in $Relative.Split('/')) {
    $current=Join-Path $current $part
    $item=Get-Item -LiteralPath $current
    if ($item.Attributes -band [IO.FileAttributes]::ReparsePoint) { throw 'Release payload links/junctions are refused.' }
  }
  return $current
}
function Get-ReleaseInventory([string]$Root) {
  Assert-ReleaseAncestors $Root | Out-Null
  $rootItem=Get-Item -LiteralPath $Root
  if (!$rootItem.PSIsContainer -or ($rootItem.Attributes -band [IO.FileAttributes]::ReparsePoint)) { throw 'The release payload must be an ordinary directory.' }
  $files=@();$pending=[Collections.Generic.Queue[string]]::new();$pending.Enqueue($rootItem.FullName)
  while ($pending.Count) {
    foreach ($item in @(Get-ChildItem -LiteralPath $pending.Dequeue() -Force)) {
      if ($item.Attributes -band [IO.FileAttributes]::ReparsePoint) { throw 'Release payload links/junctions are refused before traversal.' }
      $files+=$item
      if ($files.Count -gt 256) { throw 'Release payload has too many files.' }
      if ($item.PSIsContainer) { $pending.Enqueue($item.FullName) }
    }
  }
  $total=0L
  $records=@(foreach ($file in ($files | Where-Object { !$_.PSIsContainer } | Sort-Object FullName)) {
    $relative=$file.FullName.Substring($rootItem.FullName.TrimEnd('\','/').Length+1).Replace('\','/')
    Assert-ReleasePath $Root $relative | Out-Null
    $total+=$file.Length
    if ($file.Length -gt 128MB -or $total -gt 256MB) { throw 'Prepared release payload exceeds its size budget.' }
    [ordered]@{path=$relative;size=$file.Length;sha256=(Get-ReleaseSha256 $file.FullName)}
  })
  return $records
}
function Assert-PreparedRelease([string]$Root,$Receipt,[string]$Commit,[string]$AppVersion,[string]$Name,[string]$Publisher,[string]$IdentityVersion) {
  Assert-ReleaseIdentity $Name $Publisher $IdentityVersion
  if ($Commit -notmatch '^[a-fA-F0-9]{40}$' -or $Receipt.schema -ne 1 -or $Receipt.source_commit -cne $Commit -or $Receipt.app_version -cne $AppVersion -or $Receipt.target -cne 'x86_64-pc-windows-msvc' -or $Receipt.identity.name -cne $Name -or $Receipt.identity.publisher -cne $Publisher -or $Receipt.identity.version -cne $IdentityVersion -or $Receipt.identity.application -cne 'Pickle') { throw 'Prepared release identity/version/source metadata mismatch.' }
  if ($Receipt.identity.family -cnotmatch ('^'+[regex]::Escape($Name)+'_[a-z0-9]{13}$')) { throw 'Prepared package family is invalid.' }
  $actual=@(Get-ReleaseInventory $Root)
  $expected=@($Receipt.files)
  if ($actual.Count -ne $expected.Count) { throw 'Prepared release file inventory changed.' }
  foreach ($record in $actual) {
    $match=@($expected | Where-Object { $_.path -ceq $record.path })
    if ($match.Count -ne 1 -or $match[0].size -ne $record.size -or $match[0].sha256 -cne $record.sha256) { throw 'Prepared release bytes do not match the authenticated receipt.' }
  }
  $required=@('PickleRune.exe','pickle-browser-host.exe','PickleIdentity.msix','PickleIdentity.json','identity-registration.ps1','Pickle.ico','Pickle-preview.png','THIRD-PARTY-NOTICES.md','THIRD_PARTY_NOTICES.txt','dependency-inventory.json','browser-extension/manifest.json')
  foreach ($name in $required) { if ($name -cnotin $actual.path) { throw "Required release payload file is missing: $name" } }
  foreach ($record in $actual) {
    if ($record.path -cnotin $required -and $record.path -notmatch '^browser-extension/[A-Za-z0-9._-]+$') { throw "Unexpected release payload file: $($record.path)" }
  }
}
function Assert-ReleaseCertificate([string]$Thumbprint,[string]$Subject) {
  if ($Thumbprint -notmatch '^[0-9A-Fa-f]{40}$') { throw 'An exact existing signing certificate thumbprint is required.' }
  $certificate=Get-Item -LiteralPath ("Cert:\CurrentUser\My\"+$Thumbprint)
  if ($certificate.Subject -cne $Subject -or !$certificate.HasPrivateKey -or $certificate.NotBefore.ToUniversalTime() -gt [DateTime]::UtcNow -or $certificate.NotAfter.ToUniversalTime() -le [DateTime]::UtcNow) { throw 'The existing signing certificate subject, private key, or validity does not match.' }
  $codeSigning=$false
  foreach ($extension in $certificate.Extensions) {
    if ($extension -is [Security.Cryptography.X509Certificates.X509EnhancedKeyUsageExtension]) {
      foreach ($oid in $extension.EnhancedKeyUsages) { if ($oid.Value -eq '1.3.6.1.5.5.7.3.3') { $codeSigning=$true } }
    }
  }
  if (!$codeSigning) { throw 'The existing certificate must explicitly permit code signing.' }
  return $certificate
}
function Assert-ReleaseSignature([string]$Path,[string]$Thumbprint,[string]$Subject,[string]$SignTool) {
  & $SignTool verify /pa /all /tw /v $Path | Out-Host
  if ($LASTEXITCODE -ne 0) { throw "Trusted signature/timestamp verification failed for $([IO.Path]::GetFileName($Path))." }
  if ([IO.Path]::GetExtension($Path) -ieq '.msix') {
    # MSIX's PKCX-prefixed SignedData is inspected independently from SignTool's trust check.
    Add-Type -AssemblyName System.Security
    Add-Type -AssemblyName System.IO.Compression.FileSystem
    $archive=[IO.Compression.ZipFile]::OpenRead($Path)
    try {
      $entry=$archive.GetEntry('AppxSignature.p7x')
      if (!$entry -or $entry.Length -gt 1MB) { throw 'The signed MSIX has no bounded signature record.' }
      $signatureStream=$entry.Open();$memory=[IO.MemoryStream]::new()
      try { $signatureStream.CopyTo($memory);$bytes=$memory.ToArray() } finally { $signatureStream.Dispose();$memory.Dispose() }
      if ($bytes.Length -lt 5 -or [Text.Encoding]::ASCII.GetString($bytes,0,4) -cne 'PKCX') { throw 'Unexpected MSIX signature encoding.' }
      $cms=[Security.Cryptography.Pkcs.SignedCms]::new()
      $cms.Decode($bytes[4..($bytes.Length-1)])
      if ($cms.SignerInfos.Count -ne 1) { throw 'Expected one MSIX signing identity.' }
      $signer=$cms.SignerInfos[0]
      if ($signer.Certificate.Thumbprint -ine $Thumbprint -or $signer.Certificate.Subject -cne $Subject) { throw 'MSIX signer does not match the release identity.' }
      if (!@($signer.UnsignedAttributes | Where-Object { $_.Oid.Value -eq '1.3.6.1.4.1.311.3.3.1' -or $_.Oid.Value -eq '1.2.840.113549.1.9.16.2.14' }).Count) { throw 'MSIX has no RFC3161 timestamp attribute.' }
    } finally { $archive.Dispose() }
  } else {
    $signature=Get-AuthenticodeSignature -LiteralPath $Path
    if ($signature.Status -ne 'Valid' -or !$signature.SignerCertificate -or $signature.SignerCertificate.Thumbprint -ine $Thumbprint -or $signature.SignerCertificate.Subject -cne $Subject -or !$signature.TimeStamperCertificate) { throw 'PE signature identity/timestamp does not match the release requirements.' }
  }
  return [ordered]@{file=[IO.Path]::GetFileName($Path);size=(Get-Item -LiteralPath $Path).Length;sha256=(Get-ReleaseSha256 $Path);signer_thumbprint=$Thumbprint.ToUpperInvariant();subject=$Subject;trusted_timestamp=$true}
}
function Sign-ReleaseFile([string]$Path,$Config) {
  & $Config.signtool sign /sha1 $Config.thumbprint /s My /fd SHA256 /tr $Config.timestamp_url /td SHA256 $Path | Out-Host
  if ($LASTEXITCODE -ne 0) { throw 'Release signing failed.' }
  return (Assert-ReleaseSignature $Path $Config.thumbprint $Config.subject $Config.signtool)
}
