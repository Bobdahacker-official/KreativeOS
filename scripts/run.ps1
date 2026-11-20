param(
    [switch]$Headless,
    [ValidateSet('IDE', 'SATA', 'NVME', 'USB')][string]$Storage = 'NVME',
    [switch]$UsbInput,
    [switch]$UsbBoot,
    [ValidateRange(256, 4096)][int]$MemoryMiB = 256
)

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
    $data = Join-Path $build 'KreativeOS-exfat.img'
    if ($env:KREATIVE_DATA_IMAGE) { $data = $env:KREATIVE_DATA_IMAGE }
    if ($UsbBoot) {
        if ($Storage -ne 'USB') { throw '-UsbBoot requires -Storage USB.' }
        $data = if ($env:KREATIVE_USB_IMAGE) { $env:KREATIVE_USB_IMAGE } else { Join-Path $build 'KreativeOS-A520-usb.img' }
        $disk = $data
    }
    $serial = Join-Path $build 'serial.log'
    foreach ($file in @($code, $template, $disk, $data)) {
        if (-not (Test-Path -LiteralPath $file)) { throw "Missing $file. Build first and verify your OVMF paths." }
    }
    foreach ($file in @($disk, $data)) {
        $item = Get-Item -LiteralPath $file -ErrorAction Stop
        if ($file.StartsWith('\\.\') -or $file.StartsWith('\\?\') -or
            $item.PSIsContainer -or $item.LinkType -or $item.Extension -ne '.img') {
            throw 'Use a local .img file for guest storage.'
        }
    }
    if (-not (Test-Path -LiteralPath $vars)) { Copy-Item -LiteralPath $template -Destination $vars }
    $nic = if ($env:NIC_MODEL) { $env:NIC_MODEL } else { 'rtl8139' }
    $machine = if ($UsbInput) { 'pc,i8042=off' } else { 'pc' }
    $options = @(
        '-machine', $machine, '-cpu', 'max', '-m', "$MemoryMiB",
        '-drive', "if=pflash,format=raw,readonly=on,file=$code",
        '-drive', "if=pflash,format=raw,file=$vars",
        '-device', "$nic,netdev=net0", '-netdev', 'user,id=net0',
        '-vga', 'std', '-serial', "file:$serial",
        '-audiodev', 'none,id=kos_audio', '-device', 'AC97,audiodev=kos_audio'
    )
    if (-not $UsbBoot) {
        $options += @('-drive', "if=none,id=kos,format=raw,file=$disk", '-device', 'virtio-blk-pci,drive=kos,bootindex=1')
    }
    if ($UsbInput -or $Storage -eq 'USB') {
        $options += @('-device', 'qemu-xhci,id=kosusb')
    }
    if ($UsbInput) {
        $options += @('-device', 'usb-kbd,bus=kosusb.0,port=1', '-device', 'usb-mouse,bus=kosusb.0,port=2')
    }
    if ($Storage -eq 'SATA') {
        $options += @('-device', 'ich9-ahci,id=kossata', '-drive', "if=none,id=kosdata,format=raw,file=$data",
                      '-device', 'ide-hd,drive=kosdata,bus=kossata.0')
    } elseif ($Storage -eq 'NVME') {
        $options += @('-drive', "if=none,id=kosdata,format=raw,file=$data",
                      '-device', 'nvme,drive=kosdata,serial=KOSDATA001')
    } elseif ($Storage -eq 'USB') {
        $bootOrder = if ($UsbBoot) { ',bootindex=1' } else { '' }
        $options += @('-drive', "if=none,id=kosdata,format=raw,file=$data",
                      '-device', "usb-storage,bus=kosusb.0,port=3,drive=kosdata$bootOrder")
    } else {
        $options += @('-drive', "if=ide,index=0,format=raw,file=$data")
    }
    if ($Headless) { $options += @('-display', 'none', '-no-reboot') }
    & $qemu @options
    if ($LASTEXITCODE -ne 0) { throw "QEMU exited with code $LASTEXITCODE." }
} finally {
    Pop-Location
}
