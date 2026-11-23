param(
  [ValidateSet('debug','release')][string]$Configuration = 'debug',
  [string]$ExpectedVersion
)
# This changes trust only inside a disposable hosted Windows CI runner.
# The production builders never create certificates or modify trust stores.
$ErrorActionPreference = 'Stop'
if ([Environment]::OSVersion.Platform -ne [PlatformID]::Win32NT -or $env:GITHUB_ACTIONS -cne 'true' -or $env:RUNNER_ENVIRONMENT -cne 'github-hosted') {
  throw 'This test is restricted to disposable GitHub-hosted Windows runners.'
}
# Appx and the deployment helper's WinRT projection use the inbox Windows PowerShell runtime.
# Modern pwsh/.NET removed built-in WinRT activation; keep the entire cleanup in the same shell.
if ($PSEdition -eq 'Core') {
  $delegateArguments = @('-NoLogo','-NoProfile','-NonInteractive','-ExecutionPolicy','Bypass','-File',$PSCommandPath,'-Configuration',$Configuration)
  if ($ExpectedVersion) { $delegateArguments += @('-ExpectedVersion',$ExpectedVersion) }
  & "$env:WINDIR/System32/WindowsPowerShell/v1.0/powershell.exe" @delegateArguments
  if ($LASTEXITCODE -ne 0) { throw "Signed identity CI failed in Windows PowerShell with status $LASTEXITCODE." }
  return
}
# Appx deployment validates machine trust, not just the current user's store.
# https://learn.microsoft.com/windows/msix/package/create-certificate-package-signing
# Require the disposable runner's existing elevated token; never request elevation.
$identity = [Security.Principal.WindowsIdentity]::GetCurrent()
try {
  $principal = New-Object Security.Principal.WindowsPrincipal($identity)
  if (!$principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
    throw 'The disposable hosted Windows runner must already be elevated for its test-only machine TrustedPeople certificate.'
  }
} finally { $identity.Dispose() }
Set-Location (Join-Path $PSScriptRoot '..')
$uninstallKey = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\PickleRune'
if (Test-Path -LiteralPath $uninstallKey) { throw 'The hosted runner already contains an installed Pickle. Refusing to change it.' }
$id = [guid]::NewGuid().ToString('N')
$name = 'Pickle.Rune.CI.' + $id.Substring(0,16)
$subject = 'CN=Pickle CI ' + $id
$work = Join-Path ([IO.Path]::GetTempPath()) ('pickle-identity-ci-' + $id)
$install = Join-Path $work 'installation'
New-Item -ItemType Directory -Path $work | Out-Null
$certificate = $null
$trustStores = @()
$stage = $null
$baseline = @{}
foreach ($file in @('PickleRune-setup.exe','PickleRune-windows.zip')) {
  $path = Join-Path $PWD ('artifacts/' + $file)
  if (Test-Path -LiteralPath $path) {
    $copy = Join-Path $work $file
    Copy-Item -LiteralPath $path -Destination $copy
    $baseline[$file] = $copy
  }
}
try {
  if ($Configuration -eq 'release') {
    # Complete all Cargo/dependency execution before creating the disposable signing key.
    $prepared=Join-Path $work 'prepared'
    $sourceCommit=(& git rev-parse HEAD).Trim()
    & ./scripts/prepare-windows-release.ps1 -PackageName $name -Publisher $subject -PublisherDisplayName 'Pickle CI test only' -IdentityVersion '0.1.0.0' -ExpectedSourceCommit $sourceCommit -OutputDirectory $prepared -Configuration release
    $receiptPath=Join-Path $prepared 'prepared-receipt.json'
    $receiptDigest=(Get-FileHash -LiteralPath $receiptPath -Algorithm SHA256).Hash.ToLowerInvariant()
    $preparedReceipt=Get-Content -Raw -LiteralPath $receiptPath | ConvertFrom-Json
  }
  $certificate = New-SelfSignedCertificate -Type Custom -KeyUsage DigitalSignature `
    -CertStoreLocation 'Cert:\CurrentUser\My' -Subject $subject -FriendlyName 'Disposable Pickle CI signing test' `
    -TextExtension @('2.5.29.37={text}1.3.6.1.5.5.7.3.3','2.5.29.19={text}') `
    -HashAlgorithm SHA256 -KeyAlgorithm RSA -KeyLength 2048 -KeyExportPolicy NonExportable -NotAfter (Get-Date).AddDays(1)
  $publicCertificate = Join-Path $work 'public-test-certificate.cer'
  Export-Certificate -Cert $certificate -FilePath $publicCertificate | Out-Null
  # TrustedPeople grants peer trust to this exact code-signing leaf. Do not put
  # a non-CA leaf into Root, and do not weaken the production SignTool /pa check.
  $trustStore = 'Cert:\LocalMachine\TrustedPeople'
  $trustPath = Join-Path $trustStore $certificate.Thumbprint
  if (Test-Path -LiteralPath $trustPath) { throw 'The new test certificate unexpectedly already exists in machine trust. It was preserved.' }
  # Record before import so finally cleans up even if Import-Certificate partly
  # succeeds and then throws. Never remove an entry that existed before this run.
  $trustStores += $trustStore
  Import-Certificate -FilePath $publicCertificate -CertStoreLocation $trustStore | Out-Null
  if (!(Test-Path -LiteralPath $trustPath)) { throw 'The exact disposable certificate was not imported into machine TrustedPeople.' }
  if ($Configuration -eq 'release') {
    $finalized=Join-Path $work 'finalized'
    & ./scripts/finalize-windows-release.ps1 -PreparedDirectory (Join-Path $prepared 'payload') -ReceiptPath $receiptPath -ExpectedReceiptSha256 $receiptDigest -ExpectedSourceCommit $sourceCommit -ExpectedAppVersion $preparedReceipt.app_version -PackageName $name -Publisher $certificate.Subject -IdentityVersion '0.1.0.0' -ExpectedPackageFamily $preparedReceipt.identity.family -CertificateThumbprint $certificate.Thumbprint -OutputDirectory $finalized
    $stage=Get-Item -LiteralPath (Join-Path $prepared 'payload')
    Copy-Item -LiteralPath (Join-Path $finalized 'PickleRune-setup.exe') -Destination artifacts/PickleRune-identity-ci-test-setup.exe
    Copy-Item -LiteralPath (Join-Path $finalized 'PickleRune-windows.zip') -Destination artifacts/PickleRune-identity-ci-test-windows.zip
    Copy-Item -LiteralPath (Join-Path $finalized 'release-report.json') -Destination artifacts/identity-ci-test-release-report.json
    # Staged app/installer execution starts only after this test private key has been removed.
    $privatePath=Join-Path 'Cert:\CurrentUser\My' $certificate.Thumbprint
    Remove-Item -Path $privatePath -DeleteKey -ErrorAction Stop
    if (Test-Path -LiteralPath $privatePath) { throw 'The disposable signing key remains before staged application execution.' }
    $publicThumbprint=$certificate.Thumbprint;$publicSubject=$certificate.Subject
    $certificate.Dispose()
    $certificate=[pscustomobject]@{Thumbprint=$publicThumbprint;Subject=$publicSubject}
  } else {
    # Preserve the established developer/debug path; production release uses isolation above.
    & ./scripts/build-identity.ps1 -Configuration $Configuration -PackageName $name -Publisher $certificate.Subject `
      -PublisherDisplayName 'Pickle CI test only' -Version '0.1.0.0' -CertificateThumbprint $certificate.Thumbprint -Installer
    $stage = Get-ChildItem artifacts/windows-* -Directory | Where-Object {
      $metadata = Join-Path $_.FullName 'PickleIdentity.json'
      (Test-Path -LiteralPath $metadata) -and (Get-Content -Raw -LiteralPath $metadata | ConvertFrom-Json).name -ceq $name
    } | Sort-Object LastWriteTime -Descending | Select-Object -First 1
    if (!$stage) { throw 'The signed build did not produce this test identity payload.' }
    Copy-Item -LiteralPath artifacts/PickleRune-setup.exe -Destination artifacts/PickleRune-identity-ci-test-setup.exe
    Copy-Item -LiteralPath artifacts/PickleRune-windows.zip -Destination artifacts/PickleRune-identity-ci-test-windows.zip
  }
  & ./scripts/test-windows-package.ps1 -InstallerPath artifacts/PickleRune-identity-ci-test-setup.exe -InstallDirectory $install -Configuration $Configuration -ExpectedVersion $ExpectedVersion
}
finally {
  try {
    # Retry only this test's uninstaller, and only when it owns the installed-app entry.
    $uninstaller = Join-Path $install 'Uninstall.exe'
    if ((Test-Path -LiteralPath $uninstaller) -and (Test-Path -LiteralPath $uninstallKey) -and
        (Get-ItemProperty -LiteralPath $uninstallKey).InstallLocation -eq $install) {
      $cleanup = Start-Process -FilePath $uninstaller -ArgumentList @('/S', "_?=$install") -Wait -PassThru
      if ($cleanup.ExitCode -ne 0) { Write-Warning 'The test uninstaller failed; attempting exact identity cleanup below.' }
    }
    if ($stage -and (Test-Path -LiteralPath $install)) {
      . ./packaging/windows/identity-registration.ps1
      Invoke-PickleIdentity Unregister $install (Join-Path $stage.FullName 'PickleIdentity.json')
    }
    if ($certificate -and @(Get-AppxPackage -Name $name | Where-Object { $_.Name -ceq $name -and $_.Publisher -ceq $certificate.Subject }).Count) {
      throw 'The exact CI identity is still registered after cleanup.'
    }
  }
  finally {
    $cleanupErrors = @()
    if ($certificate) {
      foreach ($store in ($trustStores + @('Cert:\CurrentUser\My'))) {
        $path = Join-Path $store $certificate.Thumbprint
        try {
          if (Test-Path -LiteralPath $path) {
            if ($store -eq 'Cert:\CurrentUser\My') { Remove-Item -Path $path -DeleteKey -ErrorAction Stop }
            else { Remove-Item -Path $path -ErrorAction Stop }
            if (Test-Path -LiteralPath $path) { throw 'The exact disposable certificate remains after removal.' }
          }
        } catch { $cleanupErrors += "Could not remove this test certificate from $store`: $_" }
      }
    }
    # Keep test-signed installers clearly named; retain the ordinary development artifacts.
    foreach ($file in $baseline.Keys) {
      try { Copy-Item -LiteralPath $baseline[$file] -Destination (Join-Path $PWD ('artifacts/' + $file)) -Force -ErrorAction Stop }
      catch { $cleanupErrors += "Could not restore ordinary artifact $file`: $_" }
    }
    if ($cleanupErrors.Count) { throw ($cleanupErrors -join [Environment]::NewLine) }
  }
}
Write-Host "PASS ($Configuration): hosted-runner test signing, installed external identity/version, real fixed-fixture OCR, explicit UI smoke, repair, uninstall, and exact certificate/private-key cleanup."
