param(
  [Parameter(Mandatory=$true)][string]$PackageDirectory,
  [Parameter(Mandatory=$true)][ValidateSet('chrome','edge')][string]$Browser,
  [Parameter(Mandatory=$true)][string]$EvidencePath
)
$ErrorActionPreference='Stop'
if ($PSEdition -cne 'Core' -or !$IsWindows -or $env:GITHUB_ACTIONS -cne 'true' -or $env:RUNNER_ENVIRONMENT -cne 'github-hosted') { throw 'This dedicated native setup entry runs only in PowerShell 7 on hosted Windows CI.' }
. (Join-Path $PSScriptRoot 'windows-reading-containment.ps1')
# No profile/extension mutation or subprocess before whole-owner containment.
Initialize-PickleReadingDelegateContainment
. (Join-Path $PSScriptRoot 'windows-extension-smoke.ps1')
Invoke-PickleExtensionSetupStage $PackageDirectory $Browser $EvidencePath
