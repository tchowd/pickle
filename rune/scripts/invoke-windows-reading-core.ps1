param(
  [Parameter(Mandatory=$true)][string]$Application,
  [Parameter(Mandatory=$true)][string]$DataDirectory,
  [Parameter(Mandatory=$true)][string]$EvidencePath,
  [Parameter(Mandatory=$true)][ValidateSet('debug','release')][string]$Configuration
)
$ErrorActionPreference='Stop'
if ($PSEdition -cne 'Core' -or $PSVersionTable.PSVersion.Major -lt 7 -or [Environment]::OSVersion.Platform -ne [PlatformID]::Win32NT) { throw 'Installed reading acceptance requires PowerShell 7 on Windows.' }
. (Join-Path $PSScriptRoot 'windows-reading-containment.ps1')
# Establish containment before compiling helpers or touching the app profile.
# Keep its raw static handle open until this dedicated -File process exits.
Initialize-PickleReadingDelegateContainment
. (Join-Path $PSScriptRoot 'windows-reading-smoke.ps1')
& (Join-Path $PSScriptRoot 'test-windows-reading-smoke.ps1')
Assert-PickleInstalledReadingWorkflow $Application $DataDirectory $EvidencePath $Configuration
