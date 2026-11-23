param(
  [Parameter(Mandatory)][string]$PackageName,
  [Parameter(Mandatory)][string]$Publisher,
  [Parameter(Mandatory)][string]$PublisherDisplayName,
  [Parameter(Mandatory)][string]$IdentityVersion,
  [Parameter(Mandatory)][string]$ExpectedSourceCommit,
  [Parameter(Mandatory)][string]$OutputDirectory,
  [ValidateSet('debug','release')][string]$Configuration='release'
)
$ErrorActionPreference='Stop'
. (Join-Path $PSScriptRoot 'windows-release-common.ps1')
if ([Environment]::OSVersion.Platform -ne [PlatformID]::Win32NT) { throw 'Windows identity preparation requires Windows SDK tools.' }
Assert-ReleaseIdentity $PackageName $Publisher $IdentityVersion
if ($env:PICKLE_SIGN_CERT_THUMBPRINT) { throw 'Unsigned preparation refuses signing configuration. Run this build job without signing keys/credentials.' }
$root=Split-Path $PSScriptRoot -Parent
Set-Location $root
$commit=(& git rev-parse HEAD).Trim()
if ($LASTEXITCODE -ne 0 -or $ExpectedSourceCommit -notmatch '^[a-fA-F0-9]{40}$' -or $commit -cne $ExpectedSourceCommit -or (& git status --porcelain --untracked-files=normal)) { throw 'Unsigned preparation requires the exact expected clean source commit.' }
$metadata=(& cargo metadata --locked --no-deps --format-version 1 | ConvertFrom-Json)
if ($LASTEXITCODE -ne 0) { throw 'Could not inspect release package metadata.' }
$appVersion=($metadata.packages | Where-Object { $_.name -eq 'pickle-app' }).version
if (!$appVersion -or !$PublisherDisplayName) { throw 'App version and publisher display name are required.' }
if (Test-Path -LiteralPath $OutputDirectory) { throw 'Use a new unsigned preparation directory.' }
Assert-ReleaseAncestors $OutputDirectory -MayBeMissing | Out-Null
$OutputDirectory=[IO.Path]::GetFullPath($OutputDirectory)
New-Item -ItemType Directory -Path $OutputDirectory | Out-Null
$identityRoot=Join-Path $OutputDirectory 'identity'
New-Item -ItemType Directory -Path $identityRoot | Out-Null
[xml]$manifest=Get-Content -Raw (Join-Path $root 'packaging/windows/IdentityManifest.xml')
$manifest.Package.Identity.SetAttribute('Name',$PackageName)
$manifest.Package.Identity.SetAttribute('Publisher',$Publisher)
$manifest.Package.Identity.SetAttribute('Version',$IdentityVersion)
$manifest.Package.Properties.PublisherDisplayName=$PublisherDisplayName
$manifest.Save((Join-Path $identityRoot 'AppxManifest.xml'))
Copy-Item (Join-Path $root 'packaging/assets/Pickle-preview.png') $identityRoot
[xml]$pe=Get-Content -Raw (Join-Path $root 'packaging/windows/app.manifest')
$msix=$pe.CreateElement('msix','urn:schemas-microsoft-com:msix.v1')
$msix.SetAttribute('publisher',$Publisher);$msix.SetAttribute('packageName',$PackageName);$msix.SetAttribute('applicationId','Pickle')
$pe.DocumentElement.AppendChild($msix) | Out-Null
$pePath=Join-Path $OutputDirectory 'application.manifest';$pe.Save($pePath)
$makeappx=Find-WindowsSdkTool 'makeappx.exe'
$package=Join-Path $OutputDirectory 'PickleIdentity.msix'
& $makeappx pack /o /d $identityRoot /nv /p $package
if ($LASTEXITCODE -ne 0) { throw 'Unsigned sparse identity creation failed.' }
# Windows derives the family from the public package name and publisher, not a certificate/key.
if (-not ('PickleUnsignedReleaseIdentity' -as [type])) {
  Add-Type -TypeDefinition @'
using System; using System.Runtime.InteropServices; using System.Text;
public static class PickleUnsignedReleaseIdentity {
 [StructLayout(LayoutKind.Sequential,CharSet=CharSet.Unicode)] struct Id {
  public uint Reserved,Architecture; public ulong Version;
  [MarshalAs(UnmanagedType.LPWStr)] public string Name;
  [MarshalAs(UnmanagedType.LPWStr)] public string Publisher;
  [MarshalAs(UnmanagedType.LPWStr)] public string Resource;
  [MarshalAs(UnmanagedType.LPWStr)] public string PublisherId;
 }
 [DllImport("kernel32.dll",CharSet=CharSet.Unicode)] static extern int PackageFamilyNameFromId(ref Id id,ref uint length,StringBuilder output);
 public static string Family(string name,string publisher) {
  var id=new Id { Architecture=11,Name=name,Publisher=publisher,Resource="" }; uint length=0;
  if(PackageFamilyNameFromId(ref id,ref length,null)!=122 || length<2 || length>256) throw new InvalidOperationException("Invalid package family");
  var output=new StringBuilder((int)length);
  if(PackageFamilyNameFromId(ref id,ref length,output)!=0) throw new InvalidOperationException("Cannot read package family");
  return output.ToString();
 }
}
'@
}
$family=[PickleUnsignedReleaseIdentity]::Family($PackageName,$Publisher)
Write-ReleaseJson ([ordered]@{schema=1;name=$PackageName;publisher=$Publisher;family=$family;version=$IdentityVersion;application='Pickle';sha256=(Get-ReleaseSha256 $package)}) (Join-Path $OutputDirectory 'PickleIdentity.json')
$before=@(Get-ChildItem (Join-Path $root 'artifacts/windows-*') -Directory -ErrorAction SilentlyContinue | ForEach-Object { $_.FullName })
$savedManifest=$env:PICKLE_WINDOWS_APPLICATION_MANIFEST
$savedCommit=$env:PICKLE_BUILD_COMMIT
try {
  $env:PICKLE_WINDOWS_APPLICATION_MANIFEST=$pePath
  $env:PICKLE_BUILD_COMMIT=$commit
  & (Join-Path $PSScriptRoot 'build-windows.ps1') -Configuration $Configuration -IdentityPackage $package
} finally { $env:PICKLE_WINDOWS_APPLICATION_MANIFEST=$savedManifest;$env:PICKLE_BUILD_COMMIT=$savedCommit }
$created=@(Get-ChildItem (Join-Path $root 'artifacts/windows-*') -Directory | Where-Object { $_.FullName -notin $before })
if ($created.Count -ne 1) { throw 'Preparation could not identify exactly one new unsigned staging directory.' }
$payload=Join-Path $OutputDirectory 'payload'
Copy-Item -LiteralPath $created[0].FullName -Destination $payload -Recurse
$receipt=[ordered]@{schema=1;source_commit=$commit;app_version=$appVersion;configuration=$Configuration;target='x86_64-pc-windows-msvc';identity=[ordered]@{name=$PackageName;publisher=$Publisher;family=$family;version=$IdentityVersion;application='Pickle'};update=[ordered]@{manifest_url=$env:PICKLE_UPDATE_MANIFEST_URL;public_key_hex=$env:PICKLE_UPDATE_PUBLIC_KEY_HEX;channel=$env:PICKLE_UPDATE_CHANNEL};files=@(Get-ReleaseInventory $payload)}
$receiptPath=Join-Path $OutputDirectory 'prepared-receipt.json';Write-ReleaseJson $receipt $receiptPath
Assert-PreparedRelease $payload $receipt $commit $appVersion $PackageName $Publisher $IdentityVersion
Write-Host "Unsigned payload: $payload"
Write-Host "Receipt: $receiptPath"
Write-Host "Receipt SHA256: $(Get-ReleaseSha256 $receiptPath)"
Write-Host 'Authenticate this build artifact/digest in the release workflow. This public receipt alone does not prove source provenance or embedded update configuration.'
