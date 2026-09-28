$ErrorActionPreference = 'Stop'
Push-Location (Split-Path -Parent $PSScriptRoot)
try {
    cargo build --profile dist --locked
    if ($LASTEXITCODE -ne 0) { throw 'Release build failed.' }
    New-Item -ItemType Directory -Path dist -Force | Out-Null
    Copy-Item -LiteralPath target/dist/adeline.exe -Destination dist/Adeline.exe -Force
    # Notices are embedded and shown in Settings > Licenses.
    foreach ($notice in @('dist/PHOSPHOR-LICENSE.txt', 'dist/LOBE-LICENSE.txt')) {
        if (Test-Path -LiteralPath $notice) { Remove-Item -LiteralPath $notice -Force }
    }
    Write-Output 'Packaged release build: dist/Adeline.exe'
} finally {
    Pop-Location
}
