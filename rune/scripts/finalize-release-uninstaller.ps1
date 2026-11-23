param([Parameter(Mandatory)][string]$Path,[Parameter(Mandatory)][string]$ConfigurationFile)
$ErrorActionPreference='Stop'
. (Join-Path $PSScriptRoot 'windows-release-common.ps1')
$config=Get-Content -Raw -LiteralPath $ConfigurationFile | ConvertFrom-Json
Assert-ReleaseCertificate $config.thumbprint $config.subject | Out-Null
$verification=Sign-ReleaseFile $Path $config
# The compiler's temporary generated uninstaller is not executed; retain its exact signed bytes.
[IO.File]::Copy($Path,$config.uninstaller_copy,$false)
Write-ReleaseJson $verification $config.uninstaller_report
