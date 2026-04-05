Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$repoRoot = Split-Path -Parent $PSScriptRoot
$launchDevCmd = "D:\huanjing\Common7\Tools\LaunchDevCmd.bat"
$outputRoot = "E:\cc_myself"
$buildRoot = Join-Path $outputRoot "cc-switch"
$distDir = Join-Path $buildRoot "dist"
$targetDir = Join-Path $buildRoot "target"
$configPath = Join-Path $buildRoot "tauri.no-bundle.local.json"
$finalExePath = Join-Path $outputRoot "cc-switch.exe"
$cargoBin = Join-Path $env:USERPROFILE ".cargo\bin"
$repoDistLink = Join-Path $repoRoot "dist"

function Remove-JunctionIfExists {
  param(
    [Parameter(Mandatory = $true)]
    [string]$Path
  )

  if (-not (Test-Path $Path)) {
    return
  }

  cmd /d /c "rmdir `"$Path`"" | Out-Null
  if (Test-Path $Path) {
    Remove-Item -LiteralPath $Path -Force -Recurse
  }
}

function Stop-ProcessUsingPath {
  param(
    [Parameter(Mandatory = $true)]
    [string]$Path
  )

  $normalizedPath = [System.IO.Path]::GetFullPath($Path)
  $processes = Get-CimInstance Win32_Process -Filter "name = 'cc-switch.exe'" |
    Where-Object {
      $_.ExecutablePath -and
      [string]::Equals(
        [System.IO.Path]::GetFullPath($_.ExecutablePath),
        $normalizedPath,
        [System.StringComparison]::OrdinalIgnoreCase
      )
    }

  foreach ($process in $processes) {
    Stop-Process -Id $process.ProcessId -Force -ErrorAction Stop
  }
}

if (-not (Test-Path $launchDevCmd)) {
  throw "LaunchDevCmd.bat not found: $launchDevCmd"
}

New-Item -ItemType Directory -Force -Path $outputRoot | Out-Null
New-Item -ItemType Directory -Force -Path $buildRoot | Out-Null
New-Item -ItemType Directory -Force -Path $distDir | Out-Null
New-Item -ItemType Directory -Force -Path $targetDir | Out-Null

$overrideConfig = @"
{
  "bundle": {
    "active": false,
    "createUpdaterArtifacts": false
  }
}
"@

Set-Content -Path $configPath -Value $overrideConfig -Encoding UTF8

if (Test-Path $repoDistLink) {
  $existingDist = Get-Item -LiteralPath $repoDistLink
  if (-not ($existingDist.Attributes -band [System.IO.FileAttributes]::ReparsePoint)) {
    throw "Repository dist path already exists and is not a junction/symlink: $repoDistLink"
  }

  Remove-JunctionIfExists -Path $repoDistLink
}

cmd /d /c "mklink /J `"$repoDistLink`" `"$distDir`"" | Out-Null

$buildCommand = @(
  "`"$launchDevCmd`" -arch=x64 -host_arch=x64",
  "set `"PATH=$cargoBin;%PATH%`"",
  "set `"CARGO_TARGET_DIR=$targetDir`"",
  "pnpm tauri build --no-bundle --config `"$configPath`""
) -join " && "

Push-Location $repoRoot
try {
  cmd /c $buildCommand
  if ($LASTEXITCODE -ne 0) {
    throw "Build failed with exit code $LASTEXITCODE"
  }
} finally {
  Pop-Location

  Remove-JunctionIfExists -Path $repoDistLink
}

$builtExe = Join-Path $targetDir "release\cc-switch.exe"
if (-not (Test-Path $builtExe)) {
  throw "Built exe not found: $builtExe"
}

Stop-ProcessUsingPath -Path $finalExePath
Copy-Item -Force -LiteralPath $builtExe -Destination $finalExePath
Write-Host "Built exe: $finalExePath"
