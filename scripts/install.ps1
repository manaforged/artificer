param(
    [switch]$Help,
    [switch]$Uninstall
)

$ErrorActionPreference = 'Stop'

if ($Help) {
    Write-Output 'Usage: ./scripts/install.ps1 [-Help|-Uninstall]'
    Write-Output ''
    Write-Output 'Builds Artificer, then runs `artificer install`: the launchers, the'
    Write-Output 'recorded Cargo path, and ~/.artificer/env.ps1. It does not edit shell'
    Write-Output 'or editor configuration.'
    exit 0
}

$cargoHome = if ($env:CARGO_HOME) { $env:CARGO_HOME } else { Join-Path $HOME '.cargo' }
$appHome = Join-Path $HOME '.artificer'
$shimBin = Join-Path (Join-Path $appHome 'bin') 'cargo.exe'
$bin = Join-Path $cargoHome 'bin\artificer.exe'

if ($Uninstall) {
    if (Test-Path -LiteralPath $bin -PathType Leaf) {
        & $bin uninstall
        if ($LASTEXITCODE -ne 0) {
            throw "artificer uninstall failed with exit code $LASTEXITCODE"
        }
        if (Test-Path -LiteralPath $bin -PathType Leaf) {
            Remove-Item -LiteralPath $bin -Force
        }
    } else {
        Remove-Item -Force -ErrorAction SilentlyContinue $bin, $shimBin,
            (Join-Path $appHome 'real-cargo'), (Join-Path $appHome 'env.ps1'),
            (Join-Path $appHome 'store')
        Write-Output 'Removed Artificer launchers. The cache remains.'
    }
    exit 0
}

$root = Split-Path -Parent $PSScriptRoot
$stamp = Join-Path $appHome 'real-cargo'
$real = if ($env:ARTIFICER_REAL_CARGO) {
    $env:ARTIFICER_REAL_CARGO
} elseif (Test-Path -LiteralPath $stamp -PathType Leaf) {
    (Get-Content -Raw -LiteralPath $stamp).Trim()
} else {
    $null
}
if (-not $real -or -not (Test-Path -LiteralPath $real -PathType Leaf)) {
    $cargoCommand = Get-Command cargo.exe -CommandType Application -ErrorAction SilentlyContinue
    $real = if ($cargoCommand) { $cargoCommand.Source } else { $null }
}
if ($real -eq $shimBin) {
    $real = Join-Path $cargoHome 'bin\cargo.exe'
}
if ($real -match '[\\/]toolchains[\\/]') {
    $proxy = Join-Path $cargoHome 'bin\cargo.exe'
    if (Test-Path -LiteralPath $proxy -PathType Leaf) {
        $real = $proxy
    } else {
        throw 'Cargo resolved to a Rustup toolchain binary, not its proxy. Set ARTIFICER_REAL_CARGO to a system Cargo executable or Rustup proxy.'
    }
}
if (-not $real -or -not (Test-Path -LiteralPath $real -PathType Leaf)) {
    throw 'Cargo executable not found. Set ARTIFICER_REAL_CARGO.'
}

Write-Output "Building Artificer with $real"
& $real build --release --locked --target-dir (Join-Path $root 'target') `
    --manifest-path (Join-Path $root 'Cargo.toml')
if ($LASTEXITCODE -ne 0) {
    throw "Cargo build failed with exit code $LASTEXITCODE"
}

$env:ARTIFICER_REAL_CARGO = $real
& (Join-Path $root 'target\release\artificer.exe') install
if ($LASTEXITCODE -ne 0) {
    throw "artificer install failed with exit code $LASTEXITCODE"
}
