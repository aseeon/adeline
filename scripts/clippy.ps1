# Runs clippy the way CI does (from Zed's script/clippy.ps1). Extra arguments
# are passed through to cargo clippy.
$ErrorActionPreference = 'Stop'

$Cargo = if ($env:CARGO) { $env:CARGO } else { 'cargo' }

& $Cargo clippy @args --locked --all-targets --all-features -- --deny warnings
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
# The headless build (no `gui` feature).
& $Cargo clippy @args --locked --all-targets --no-default-features -- --deny warnings
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

# If local, run other checks if we have the tools installed.
if (-not $env:GITHUB_ACTIONS) {
    if (-not (Get-Command cargo-shear -ErrorAction SilentlyContinue)) { exit 0 }
    cargo shear --locked --deny-warnings --check-test-targets
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

    if (-not (Get-Command typos -ErrorAction SilentlyContinue)) { exit 0 }
    typos --config .config/typos.toml
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
}
