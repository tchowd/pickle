# Resolve SDK tools from ordinary PowerShell, including shells without vcvarsall.
function Find-WindowsSdkTool([string]$Name) {
  $command = Get-Command $Name -ErrorAction SilentlyContinue
  if ($command) { return $command.Source }
  $roots = @("${env:ProgramFiles(x86)}/Windows Kits/10/bin", "$env:ProgramFiles/Windows Kits/10/bin")
  foreach ($root in $roots) {
    if (!(Test-Path $root)) { continue }
    $versions = Get-ChildItem $root -Directory | Where-Object { $_.Name -match '^\d+\.\d+\.\d+\.\d+$' } | Sort-Object { [version]$_.Name } -Descending
    foreach ($version in $versions) {
      foreach ($arch in @('x64', 'arm64', 'x86')) {
        $tool = Join-Path $version.FullName "$arch/$Name"
        if (Test-Path $tool -PathType Leaf) { return $tool }
      }
    }
  }
  throw "Windows SDK tool $Name was not found. Install the Windows SDK packaging tools."
}
