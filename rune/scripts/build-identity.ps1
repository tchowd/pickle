param(
  [ValidateSet('debug','release')][string]$Configuration = 'release',
  [Parameter(Mandatory)][string]$PackageName,
  [Parameter(Mandatory)][string]$Publisher,
  [Parameter(Mandatory)][string]$PublisherDisplayName,
  [Parameter(Mandatory)][string]$Version,
  [string]$CertificateThumbprint = $env:PICKLE_SIGN_CERT_THUMBPRINT,
  [switch]$Installer
)
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'windows-sdk.ps1')
if ([Environment]::OSVersion.Platform -ne [PlatformID]::Win32NT) { throw 'Signed external identity packaging requires Windows.' }
if ($PackageName -notmatch '^[A-Za-z0-9][A-Za-z0-9.-]{1,48}[A-Za-z0-9]$' -or $Publisher -notmatch '^CN=' -or !$PublisherDisplayName) { throw 'Supply an explicit package name, exact signing-certificate subject and display name.' }
if ($Version -notmatch '^\d+\.\d+\.\d+\.\d+$' -or @($Version.Split('.') | Where-Object { [uint64]$_ -gt 65535 }).Count) { throw 'Identity version requires four integers from 0 to 65535.' }
if ($CertificateThumbprint -notmatch '^[a-fA-F0-9]{40}$') { throw 'Identity distribution requires an existing trusted signing certificate thumbprint. This script does not create or trust certificates.' }
if (-not ('PickleIdentityMetadata' -as [type])) {
  Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
using System.Text;
public static class PickleIdentityMetadata {
  [StructLayout(LayoutKind.Sequential, CharSet=CharSet.Unicode)]
  struct PackageId {
    public uint Reserved; public uint Architecture; public ulong Version;
    [MarshalAs(UnmanagedType.LPWStr)] public string Name;
    [MarshalAs(UnmanagedType.LPWStr)] public string Publisher;
    [MarshalAs(UnmanagedType.LPWStr)] public string ResourceId;
    [MarshalAs(UnmanagedType.LPWStr)] public string PublisherId;
  }
  [DllImport("kernel32.dll", CharSet=CharSet.Unicode)]
  static extern int PackageFamilyNameFromId(ref PackageId id, ref uint length, StringBuilder family);
  public static string FamilyName(string name, string publisher) {
    var id = new PackageId { Architecture=11, Name=name, Publisher=publisher, ResourceId="" };
    uint length=0;
    if (PackageFamilyNameFromId(ref id, ref length, null)!=122 || length>256 || length<2) throw new InvalidOperationException("Cannot inspect package family.");
    var family=new StringBuilder((int)length);
    if (PackageFamilyNameFromId(ref id, ref length, family)!=0) throw new InvalidOperationException("Cannot read package family.");
    return family.ToString();
  }
}
'@
}
$family = [PickleIdentityMetadata]::FamilyName($PackageName,$Publisher)
$root = Split-Path $PSScriptRoot -Parent
$work = Join-Path $root ('artifacts/identity-' + [guid]::NewGuid().ToString('N'))
$payload = Join-Path $work 'payload'
New-Item -ItemType Directory -Path $payload -Force | Out-Null
[xml]$manifest = Get-Content -Raw (Join-Path $root 'packaging/windows/IdentityManifest.xml')
$manifest.Package.Identity.SetAttribute('Name',$PackageName)
$manifest.Package.Identity.SetAttribute('Publisher',$Publisher)
$manifest.Package.Identity.SetAttribute('Version',$Version)
$manifest.Package.Properties.PublisherDisplayName = $PublisherDisplayName
$manifest.Save((Join-Path $payload 'AppxManifest.xml'))
Copy-Item (Join-Path $root 'packaging/assets/Pickle-preview.png') $payload
[xml]$pe = Get-Content -Raw (Join-Path $root 'packaging/windows/app.manifest')
$identity = $pe.CreateElement('msix','urn:schemas-microsoft-com:msix.v1')
$identity.SetAttribute('publisher',$Publisher)
$identity.SetAttribute('packageName',$PackageName)
$identity.SetAttribute('applicationId','Pickle')
$pe.DocumentElement.AppendChild($identity) | Out-Null
$pePath = Join-Path $work 'application.manifest'
$pe.Save($pePath)
$package = Join-Path $work 'PickleIdentity.msix'
$makeappx = Find-WindowsSdkTool 'makeappx.exe'
# Sparse packages intentionally reference externally installed executables; /nv is Microsoft's required mode.
& $makeappx pack /o /d $payload /nv /p $package
if ($LASTEXITCODE -ne 0) { throw 'Identity package creation failed.' }
$signtool = Find-WindowsSdkTool 'signtool.exe'
& $signtool sign /sha1 $CertificateThumbprint /fd SHA256 /tr http://timestamp.digicert.com /td SHA256 $package
if ($LASTEXITCODE -ne 0) { throw 'Identity signing failed.' }
& $signtool verify /pa /v $package
if ($LASTEXITCODE -ne 0) { throw 'Identity signature is not trusted on this build machine.' }
@{ schema=1; name=$PackageName; publisher=$Publisher; family=$family; version=$Version; application='Pickle'; sha256=(Get-FileHash -Algorithm SHA256 $package).Hash.ToLowerInvariant() } | ConvertTo-Json | Set-Content -Encoding utf8 (Join-Path $work 'PickleIdentity.json')
$savedManifest = $env:PICKLE_WINDOWS_APPLICATION_MANIFEST
$savedCertificate = $env:PICKLE_SIGN_CERT_THUMBPRINT
try {
  $env:PICKLE_WINDOWS_APPLICATION_MANIFEST = $pePath
  $env:PICKLE_SIGN_CERT_THUMBPRINT = $CertificateThumbprint
  & (Join-Path $PSScriptRoot 'build-windows.ps1') -Configuration $Configuration -Installer:$Installer -IdentityPackage $package
} finally {
  $env:PICKLE_WINDOWS_APPLICATION_MANIFEST = $savedManifest
  $env:PICKLE_SIGN_CERT_THUMBPRINT = $savedCertificate
}
Write-Host "Signed external identity package: $package"
Write-Host 'Registration is performed by the installer for the current user. No trust stores were changed by this build.'
