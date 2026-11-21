#!/usr/bin/env python3
"""One bounded headless QEMU boot and storage check."""
import os, subprocess, time, pathlib
root = pathlib.Path(__file__).resolve().parent.parent
(root / 'build/serial.log').unlink(missing_ok=True)
p = subprocess.Popen(['bash', 'scripts/run.sh', '-display', 'none', '-snapshot',
                      '-no-reboot'], cwd=root)
try:
    deadline = time.monotonic()+60
    log = root / 'build/serial.log'
    while time.monotonic()<deadline:
        if p.poll() is not None:
            raise RuntimeError('QEMU exited before desktop boot')
        if log.exists() and b'KreativeOS: desktop ready' in log.read_bytes():
            break
        time.sleep(.2)
    else:
        raise RuntimeError('Desktop did not boot within 60 seconds: '+
                           (log.read_text(errors='replace') if log.exists() else 'no serial log'))
    if os.environ.get('STORAGE') == 'sata':
        assert b'exFAT mounted through AHCI' in log.read_bytes(), 'SATA profile did not mount exFAT through AHCI'
    if os.environ.get('STORAGE') == 'nvme':
        assert b'exFAT mounted through NVMe' in log.read_bytes(), 'NVMe profile did not mount exFAT through NVMe'
    if os.environ.get('STORAGE') == 'usb':
        assert b'exFAT mounted through USB' in log.read_bytes(), 'USB profile did not mount exFAT through xHCI'
    print('PASS: UEFI boot, desktop initialization and selected storage profile')
finally:
    if p.poll() is None:
        p.terminate()
    p.wait(timeout=10)
