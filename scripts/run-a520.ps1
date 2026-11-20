param([switch]$Headless)

$ErrorActionPreference = 'Stop'
# Mirrors the board's xHCI input and uses a workspace image for all persistence.
& (Join-Path $PSScriptRoot 'run.ps1') -Storage USB -UsbInput -UsbBoot -MemoryMiB 1024 -Headless:$Headless
