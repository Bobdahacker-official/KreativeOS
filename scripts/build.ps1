param([switch]$PrepareUsb)

$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
Push-Location -LiteralPath $root
try {
    $cargo = Get-Command cargo -CommandType Application -ErrorAction SilentlyContinue
    $cargoPath = if ($cargo) { $cargo.Source } else { Join-Path $env:USERPROFILE '.cargo\bin\cargo.exe' }
    $pythonPath = $env:KREATIVE_PYTHON
    if (-not $pythonPath) {
        $python = Get-Command python -CommandType Application -All -ErrorAction SilentlyContinue |
            Where-Object { $_.Source -notlike '*\WindowsApps\*' } | Select-Object -First 1
        if ($python) { $pythonPath = $python.Source }
    }
    if (-not $pythonPath) {
        $python = Get-ChildItem (Join-Path $env:USERPROFILE 'AppData\Local\Python\pythoncore-*\python.exe') -ErrorAction SilentlyContinue |
            Sort-Object FullName -Descending | Select-Object -First 1
        if ($python) { $pythonPath = $python.FullName }
    }
    if (-not $pythonPath) { throw 'Install Python 3 or set KREATIVE_PYTHON to its executable.' }
    if (-not (Test-Path -LiteralPath $cargoPath)) { throw 'Install Rust and add the x86_64-unknown-uefi target.' }

    & $cargoPath build --release --locked --target x86_64-unknown-uefi
    if ($LASTEXITCODE -ne 0) { throw "Cargo exited with code $LASTEXITCODE." }
    $build = if ($env:KREATIVE_BUILD_DIR) { $env:KREATIVE_BUILD_DIR } else { 'build' }
    $target = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { 'target' }
    $esp = Join-Path $build 'esp\EFI\BOOT'
    New-Item -ItemType Directory -Force -Path $esp | Out-Null
    $loader = Join-Path $esp 'BOOTX64.EFI'
    Copy-Item -LiteralPath (Join-Path $target 'x86_64-unknown-uefi\release\kreativeos.efi') -Destination $loader
    & $pythonPath scripts/image.py
    if ($LASTEXITCODE -ne 0) { throw "Image builder exited with code $LASTEXITCODE." }
    if ($PrepareUsb) {
        $usbImage = if ($env:KREATIVE_USB_IMAGE) { $env:KREATIVE_USB_IMAGE } else { Join-Path $build 'KreativeOS-A520-usb.img' }
        & $pythonPath scripts/exfat_image.py $usbImage --boot-image (Join-Path $build 'KreativeOS.img')
        if ($LASTEXITCODE -ne 0) { throw 'USB image creation failed. Choose a new image path with KREATIVE_USB_IMAGE.' }
        Write-Host "Prepared $usbImage"
    }
    $hashes = foreach ($file in @((Join-Path $build 'KreativeOS.img'), $loader)) {
        $hash = (Get-FileHash -LiteralPath $file -Algorithm SHA256).Hash.ToLowerInvariant()
        "$hash  $($file.Replace('\', '/'))"
    }
    Set-Content -LiteralPath (Join-Path $build 'SHA256SUMS') -Value $hashes -Encoding Ascii
} finally {
    Pop-Location
}
