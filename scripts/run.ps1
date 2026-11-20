param([switch]$Headless, [ValidateSet('IDE', 'SATA')][string]$Storage = 'IDE')

$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
Push-Location -LiteralPath $root
try {
    $qemu = $env:KREATIVE_QEMU
    if (-not $qemu) {
        $command = Get-Command qemu-system-x86_64 -CommandType Application -ErrorAction SilentlyContinue
        $qemu = if ($command) { $command.Source } else { Join-Path $env:ProgramFiles 'qemu\qemu-system-x86_64.exe' }
    }
    if (-not (Test-Path -LiteralPath $qemu)) { throw 'Install QEMU or set KREATIVE_QEMU to its executable.' }
    $firmware = Join-Path (Split-Path $qemu -Parent) 'share'
    $code = if ($env:OVMF_CODE) { $env:OVMF_CODE } else { Join-Path $firmware 'edk2-x86_64-code.fd' }
    $template = if ($env:OVMF_VARS) { $env:OVMF_VARS } else { Join-Path $firmware 'edk2-i386-vars.fd' }
    $build = if ($env:KREATIVE_BUILD_DIR) { $env:KREATIVE_BUILD_DIR } else { 'build' }
    $vars = Join-Path $build 'OVMF_VARS.fd'
    $disk = Join-Path $build 'KreativeOS.img'
    $data = Join-Path $build $(if ($Storage -eq 'SATA') { 'KreativeOS-sata.img' } else { 'KreativeOS-data.img' })
    if ($env:KREATIVE_DATA_IMAGE) { $data = $env:KREATIVE_DATA_IMAGE }
    $serial = Join-Path $build 'serial.log'
    foreach ($file in @($code, $template, $disk, $data)) {
        if (-not (Test-Path -LiteralPath $file)) { throw "Missing $file. Build first and verify your OVMF paths." }
    }
    if (-not (Test-Path -LiteralPath $vars)) { Copy-Item -LiteralPath $template -Destination $vars }
    $nic = if ($env:NIC_MODEL) { $env:NIC_MODEL } else { 'rtl8139' }
    $options = @(
        '-machine', 'pc', '-cpu', 'max', '-m', '256',
        '-drive', "if=pflash,format=raw,readonly=on,file=$code",
        '-drive', "if=pflash,format=raw,file=$vars",
        '-drive', "if=none,id=kos,format=raw,file=$disk",
        '-device', 'virtio-blk-pci,drive=kos,bootindex=1',
        '-device', "$nic,netdev=net0", '-netdev', 'user,id=net0',
        '-vga', 'std', '-serial', "file:$serial",
        '-audiodev', 'none,id=kos_audio', '-device', 'AC97,audiodev=kos_audio'
    )
    if ($Storage -eq 'SATA') {
        $options += @('-device', 'ich9-ahci,id=kossata', '-drive', "if=none,id=kosdata,format=raw,file=$data",
                      '-device', 'ide-hd,drive=kosdata,bus=kossata.0')
    } else {
        $options += @('-drive', "if=ide,index=0,format=raw,file=$data")
    }
    if ($Headless) { $options += @('-display', 'none', '-no-reboot') }
    & $qemu @options
    if ($LASTEXITCODE -ne 0) { throw "QEMU exited with code $LASTEXITCODE." }
} finally {
    Pop-Location
}
