param(
  [ValidateSet('Check','Register','Unregister')][string]$Action,
  [string]$InstallDirectory,
  [string]$MetadataPath = (Join-Path $PSScriptRoot 'PickleIdentity.json')
)
$ErrorActionPreference = 'Stop'

function Test-IdentityOwner($Package, $Metadata, [string]$ExternalPath, [string]$ExpectedPath) {
  return $Package.Name -ceq $Metadata.name -and $Package.Publisher -ceq $Metadata.publisher -and
    $Package.PackageFamilyName -ceq $Metadata.family -and $ExternalPath -and
    [string]::Equals([IO.Path]::GetFullPath($ExternalPath).TrimEnd('\','/'), [IO.Path]::GetFullPath($ExpectedPath).TrimEnd('\','/'), [StringComparison]::OrdinalIgnoreCase)
}

function Get-IdentityExternalPath($Package) {
  [Windows.Management.Deployment.PackageManager,Windows.Management.Deployment,ContentType=WindowsRuntime] | Out-Null
  $manager = New-Object Windows.Management.Deployment.PackageManager
  $registered = $manager.FindPackageForUser('', $Package.PackageFullName)
  # Do not substitute InstallLocation: that is the tiny package, not its external executable directory.
  return $registered.EffectiveExternalLocation.Path
}

function Invoke-PickleIdentity([string]$Mode, [string]$Directory, [string]$MetadataFile) {
  $metadataItem = Get-Item -LiteralPath $MetadataFile
  if ($metadataItem.Length -gt 16384 -or ($metadataItem.Attributes -band [IO.FileAttributes]::ReparsePoint)) { throw 'Unsafe identity metadata.' }
  $metadata = Get-Content -Raw -LiteralPath $MetadataFile | ConvertFrom-Json
  if ($metadata.schema -ne 1 -or $metadata.name -notmatch '^[A-Za-z0-9][A-Za-z0-9.-]{1,48}[A-Za-z0-9]$' -or $metadata.publisher -notmatch '^CN=' -or $metadata.family -cnotmatch ('^' + [regex]::Escape($metadata.name) + '_[a-z0-9]{13}$') -or $metadata.application -cne 'Pickle' -or $metadata.sha256 -notmatch '^[a-f0-9]{64}$' -or $metadata.version -notmatch '^\d+\.\d+\.\d+\.\d+$') { throw 'Invalid identity metadata.' }
  $version = [version]$metadata.version
  $directoryItem = Get-Item -LiteralPath $Directory
  if (!$directoryItem.PSIsContainer -or ($directoryItem.Attributes -band [IO.FileAttributes]::ReparsePoint)) { throw 'External application directory must be an ordinary directory.' }
  $Directory = $directoryItem.FullName
  $packages = @(Get-AppxPackage -Name $metadata.name | Where-Object { $_.Name -ceq $metadata.name })
  foreach ($package in $packages) {
    $external = Get-IdentityExternalPath $package
    if (!(Test-IdentityOwner $package $metadata $external $Directory)) { throw 'This identity belongs to another publisher or installation. Nothing was unregistered.' }
    if ($Mode -ne 'Unregister' -and [version]$package.Version -gt $version) { throw 'Identity downgrade is refused.' }
  }
  if ($Mode -eq 'Check') { return }
  if ($Mode -eq 'Unregister') {
    foreach ($package in $packages) { Remove-AppxPackage -Package $package.PackageFullName -ErrorAction Stop }
    return
  }
  $packagePath = Join-Path $Directory 'PickleIdentity.msix'
  if ((Get-FileHash -LiteralPath $packagePath -Algorithm SHA256).Hash.ToLowerInvariant() -cne $metadata.sha256) { throw 'Identity package checksum mismatch.' }
  # An existing matching version at this exact external location is already registered.
  if (@($packages | Where-Object { [version]$_.Version -eq $version }).Count) { return }
  Add-AppxPackage -Path $packagePath -ExternalLocation $Directory -ErrorAction Stop
  $registered = @(Get-AppxPackage -Name $metadata.name | Where-Object { $_.Name -ceq $metadata.name -and [version]$_.Version -eq $version })
  if ($registered.Count -ne 1 -or !(Test-IdentityOwner $registered[0] $metadata (Get-IdentityExternalPath $registered[0]) $Directory)) {
    # Roll back only a newly registered, exact owned identity; never guess at another package.
    foreach ($package in $registered) {
      if (Test-IdentityOwner $package $metadata (Get-IdentityExternalPath $package) $Directory) { Remove-AppxPackage -Package $package.PackageFullName -ErrorAction Stop }
    }
    throw 'External identity did not resolve to this exact installation; owned registration was rolled back.'
  }
}

if ($Action) {
  try { Invoke-PickleIdentity $Action $InstallDirectory $MetadataPath; exit 0 }
  catch { Write-Error $_; exit 1 }
}
