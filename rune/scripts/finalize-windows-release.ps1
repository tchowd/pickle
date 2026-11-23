param(
  [Parameter(Mandatory)][string]$PreparedDirectory,
  [Parameter(Mandatory)][string]$ReceiptPath,
  [Parameter(Mandatory)][string]$ExpectedReceiptSha256,
  [Parameter(Mandatory)][string]$ExpectedSourceCommit,
  [Parameter(Mandatory)][string]$ExpectedAppVersion,
  [Parameter(Mandatory)][string]$PackageName,
  [Parameter(Mandatory)][string]$Publisher,
  [Parameter(Mandatory)][string]$IdentityVersion,
  [Parameter(Mandatory)][string]$ExpectedPackageFamily,
  [Parameter(Mandatory)][string]$CertificateThumbprint,
  [Parameter(Mandatory)][string]$OutputDirectory,
  [string]$TimestampUrl='http://timestamp.digicert.com'
)
$ErrorActionPreference='Stop'
. (Join-Path $PSScriptRoot 'windows-release-common.ps1')
if ([Environment]::OSVersion.Platform -ne [PlatformID]::Win32NT) { throw 'Windows release finalization requires Windows.' }
Assert-ReleaseAncestors $PreparedDirectory | Out-Null
Assert-ReleaseAncestors $ReceiptPath | Out-Null
Assert-ReleaseAncestors $OutputDirectory -MayBeMissing | Out-Null
$receiptItem=Get-Item -LiteralPath $ReceiptPath
if ($receiptItem.PSIsContainer -or $receiptItem.Length -gt 256KB -or ($receiptItem.Attributes -band [IO.FileAttributes]::ReparsePoint)) { throw 'Unsafe release receipt file.' }
if ($ExpectedReceiptSha256 -notmatch '^[a-fA-F0-9]{64}$' -or (Get-ReleaseSha256 $ReceiptPath) -ine $ExpectedReceiptSha256) { throw 'The release receipt does not match its independently authenticated expected digest.' }
$receipt=Get-Content -Raw -LiteralPath $ReceiptPath | ConvertFrom-Json
Assert-PreparedRelease $PreparedDirectory $receipt $ExpectedSourceCommit $ExpectedAppVersion $PackageName $Publisher $IdentityVersion
if ($receipt.configuration -cne 'release' -or $receipt.identity.family -cne $ExpectedPackageFamily) { throw 'Expected an optimized prepared payload with the exact independently supplied package family.' }
$certificate=Assert-ReleaseCertificate $CertificateThumbprint $Publisher
$timestamp=[uri]$TimestampUrl
if (!$timestamp.IsAbsoluteUri -or $timestamp.Scheme -notin @('http','https') -or $timestamp.UserInfo -or $timestamp.Fragment) { throw 'Invalid RFC3161 timestamp endpoint.' }
if (Test-Path -LiteralPath $OutputDirectory) { throw 'Use a new final release output directory.' }
$OutputDirectory=[IO.Path]::GetFullPath($OutputDirectory)
if ($OutputDirectory.StartsWith([IO.Path]::GetFullPath($PreparedDirectory).TrimEnd('\','/')+[IO.Path]::DirectorySeparatorChar,[StringComparison]::OrdinalIgnoreCase)) { throw 'Final release output must be separate from its prepared input.' }
New-Item -ItemType Directory -Path $OutputDirectory | Out-Null
$destinationRoot=$OutputDirectory
$OutputDirectory=Join-Path $destinationRoot ('.finalize-'+[guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $OutputDirectory | Out-Null
try {
$payload=Join-Path $OutputDirectory 'payload'
New-Item -ItemType Directory -Path $payload | Out-Null
foreach ($record in $receipt.files) {
  $source=Assert-ReleasePath $PreparedDirectory $record.path
  $destination=Join-Path $payload $record.path
  New-Item -ItemType Directory -Path (Split-Path $destination -Parent) -Force | Out-Null
  Copy-Item -LiteralPath $source -Destination $destination
}
Assert-PreparedRelease $payload $receipt $ExpectedSourceCommit $ExpectedAppVersion $PackageName $Publisher $IdentityVersion
$root=Split-Path $PSScriptRoot -Parent
$signtool=Find-WindowsSdkTool 'signtool.exe'
$makeappx=Find-WindowsSdkTool 'makeappx.exe'
$mt=Find-WindowsSdkTool 'mt.exe'
# This performs static PE inspection only. No cargo, build script, staged EXE or installer runs here.
& python (Join-Path $root 'packaging/verify-windows-runtime.py') --directory $payload --architecture x64 --output (Join-Path $OutputDirectory 'windows-runtime-report.json')
if ($LASTEXITCODE -ne 0) { throw 'Prepared PE runtime audit failed.' }
$app=Join-Path $payload 'PickleRune.exe'
if ([Diagnostics.FileVersionInfo]::GetVersionInfo($app).ProductVersion -cne $ExpectedAppVersion) { throw 'The static application product version differs from the expected release.' }
$peManifest=Join-Path $OutputDirectory 'application.manifest'
& $mt "-inputresource:$app;#1" "-out:$peManifest"
if ($LASTEXITCODE -ne 0) { throw 'Could not inspect the prepared application PE manifest.' }
[xml]$pe=Get-Content -Raw $peManifest
$ns=[Xml.XmlNamespaceManager]::new($pe.NameTable);$ns.AddNamespace('m','urn:schemas-microsoft-com:msix.v1')
$identity=$pe.SelectSingleNode('//m:msix',$ns)
if (!$identity -or $identity.packageName -cne $PackageName -or $identity.publisher -cne $Publisher -or $identity.applicationId -cne 'Pickle') { throw 'Prepared PE package identity mismatch.' }
$unpacked=Join-Path $OutputDirectory 'identity-inspection'
# Sparse identity intentionally references the separately inventoried external EXE.
# /nv applies to unpack too; otherwise MakeAppx requires that EXE inside the tiny package.
# Explicit identity/runtime/capability checks below and signature/timestamp gates still apply.
& $makeappx unpack /nv /p (Join-Path $payload 'PickleIdentity.msix') /d $unpacked
if ($LASTEXITCODE -ne 0) { throw 'Could not inspect the unsigned identity package.' }
[xml]$manifest=Get-Content -Raw (Join-Path $unpacked 'AppxManifest.xml')
if ($manifest.Package.Identity.Name -cne $PackageName -or $manifest.Package.Identity.Publisher -cne $Publisher -or $manifest.Package.Identity.Version -cne $IdentityVersion -or $manifest.Package.Applications.Application.Id -cne 'Pickle' -or $manifest.Package.Applications.Application.Executable -cne 'PickleRune.exe') { throw 'Prepared MSIX identity/version mismatch.' }
$namespace=[Xml.XmlNamespaceManager]::new($manifest.NameTable)
$namespace.AddNamespace('a','http://schemas.microsoft.com/appx/manifest/foundation/windows10')
$namespace.AddNamespace('u','http://schemas.microsoft.com/appx/manifest/uap/windows10/10')
$namespace.AddNamespace('r','http://schemas.microsoft.com/appx/manifest/foundation/windows10/restrictedcapabilities')
$external=$manifest.SelectSingleNode('/a:Package/a:Properties/u:AllowExternalContent',$namespace)
$application=$manifest.Package.Applications.Application
if ($manifest.Package.Identity.ProcessorArchitecture -cne 'neutral' -or !$external -or $external.InnerText -cne 'true' -or $application.GetAttribute('RuntimeBehavior','http://schemas.microsoft.com/appx/manifest/uap/windows10/10') -cne 'win32App' -or $application.GetAttribute('TrustLevel','http://schemas.microsoft.com/appx/manifest/uap/windows10/10') -cne 'mediumIL' -or $manifest.SelectNodes('/a:Package/a:Applications/a:Application',$namespace).Count -ne 1) { throw 'Prepared package is not the expected sparse desktop identity.' }
foreach ($capability in @('runFullTrust','unvirtualizedResources')) { if (!$manifest.SelectSingleNode("/a:Package/a:Capabilities/r:Capability[@Name='$capability']",$namespace)) { throw 'Prepared sparse identity lacks required capabilities.' } }
$metadata=Get-Content -Raw (Join-Path $payload 'PickleIdentity.json') | ConvertFrom-Json
if ($metadata.name -cne $PackageName -or $metadata.publisher -cne $Publisher -or $metadata.version -cne $IdentityVersion -or $metadata.family -cne $receipt.identity.family -or $metadata.application -cne 'Pickle') { throw 'Prepared native identity registration metadata mismatch.' }
foreach ($name in @('PickleRune.exe','pickle-browser-host.exe')) {
  if ((Get-AuthenticodeSignature -LiteralPath (Join-Path $payload $name)).Status -ne 'NotSigned') { throw 'Finalization requires unsigned prepared PE images.' }
}
Add-Type -AssemblyName System.IO.Compression.FileSystem
$unsigned=[IO.Compression.ZipFile]::OpenRead((Join-Path $payload 'PickleIdentity.msix'))
try { if ($unsigned.GetEntry('AppxSignature.p7x')) { throw 'Finalization requires an unsigned identity package.' } } finally { $unsigned.Dispose() }
$config=[ordered]@{signtool=$signtool;thumbprint=$certificate.Thumbprint;subject=$Publisher;timestamp_url=$TimestampUrl;uninstaller_copy=(Join-Path $OutputDirectory 'Uninstall-verification.exe');uninstaller_report=(Join-Path $OutputDirectory 'uninstaller-signature.json')}
$signatures=@()
foreach ($name in @('PickleRune.exe','pickle-browser-host.exe','PickleIdentity.msix')) { $signatures+=Sign-ReleaseFile (Join-Path $payload $name) $config }
$metadata.sha256=Get-ReleaseSha256 (Join-Path $payload 'PickleIdentity.msix')
Write-ReleaseJson $metadata (Join-Path $payload 'PickleIdentity.json')
$configPath=Join-Path $OutputDirectory 'uninstaller-signing.json';Write-ReleaseJson $config $configPath
$makensis=(Get-Command makensis.exe -ErrorAction SilentlyContinue).Source
if (!$makensis) { $makensis="${env:ProgramFiles(x86)}/NSIS/makensis.exe" }
$installer=Join-Path $OutputDirectory 'PickleRune-setup.exe'
# Compile only the checked-in reviewed installer. Avoid NSIS expansion/injection in config paths.
foreach ($path in @($payload,$installer,$configPath,$PSScriptRoot)) { if ($path -match '["$\r\n]') { throw 'Unsupported NSIS release path characters.' } }
& $makensis "/DPICKLE_STAGE=$payload" "/DPICKLE_OUTPUT=$installer" "/DPICKLE_VERSION=$ExpectedAppVersion" "/DPICKLE_RELEASE_FINALIZE_CONFIG=$configPath" "/DPICKLE_RELEASE_FINALIZE_SCRIPT=$PSScriptRoot/finalize-release-uninstaller.ps1" (Join-Path $root 'packaging/windows/PickleRune.nsi')
if ($LASTEXITCODE -ne 0) { throw 'Reviewed NSIS release compilation failed.' }
$signatures+=Assert-ReleaseSignature $config.uninstaller_copy $certificate.Thumbprint $Publisher $signtool
$signatures+=Sign-ReleaseFile $installer $config
$archive=Join-Path $OutputDirectory 'PickleRune-windows.zip'
Compress-Archive -Path (Join-Path $payload '*') -DestinationPath $archive
foreach ($path in @($installer,$archive)) { Assert-ReleaseArtifactSize (Get-Item -LiteralPath $path).Length }
$artifacts=@(foreach ($path in @($installer,$archive)) { [ordered]@{filename=[IO.Path]::GetFileName($path);size=(Get-Item -LiteralPath $path).Length;sha256=(Get-ReleaseSha256 $path)} })
Write-ReleaseJson ([ordered]@{schema=1;source_commit=$ExpectedSourceCommit;app_version=$ExpectedAppVersion;target=$receipt.target;identity=$receipt.identity;prepared_receipt_sha256=$ExpectedReceiptSha256;provenance_boundary='Receipt binds bytes and metadata; workflow must authenticate build provenance/digest independently.';signatures=$signatures;artifacts=$artifacts}) (Join-Path $OutputDirectory 'release-report.json')
Publish-ReleaseFiles $OutputDirectory $destinationRoot @('PickleRune-setup.exe','PickleRune-windows.zip','release-report.json','windows-runtime-report.json','application.manifest','uninstaller-signature.json')
} finally {
  if (Test-Path -LiteralPath $OutputDirectory) { Remove-Item -LiteralPath $OutputDirectory -Recurse -Force -ErrorAction Stop }
}
Write-Host "Signed release candidates and report: $destinationRoot. No applications were executed and nothing was published."
