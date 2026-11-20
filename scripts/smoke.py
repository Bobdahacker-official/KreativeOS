#!/usr/bin/env python3
"""One bounded QEMU boot check, with a captured desktop frame."""
import json, select, subprocess, time, pathlib, struct, zlib
root = pathlib.Path(__file__).resolve().parent.parent
(root / 'build/serial.log').unlink(missing_ok=True)
p = subprocess.Popen(['bash', 'scripts/run.sh', '-display', 'none', '-qmp',
                      'stdio', '-no-reboot'], cwd=root, stdin=subprocess.PIPE,
                     stdout=subprocess.PIPE, bufsize=0)
def message():
    if not select.select([p.stdout], [], [], 5)[0]:
        raise TimeoutError('No QMP response')
    line = p.stdout.readline()
    if not line:
        raise RuntimeError('QEMU closed QMP')
    return json.loads(line)
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
    message()
    def command(name, args=None):
        p.stdin.write((json.dumps({'execute':name, 'arguments':args or {}})+'\n').encode())
        p.stdin.flush()
        while True:
            msg = message()
            if 'error' in msg: raise RuntimeError(msg['error'])
            if 'return' in msg: return msg['return']
    command('qmp_capabilities')
    ppm = root / 'build/desktop.ppm'
    command('screendump', {'filename':str(ppm)})
    raw = ppm.read_bytes()
    header, width_height, maxval, pixels = raw.split(b'\n', 3)
    width, height = map(int, width_height.split())
    assert header == b'P6' and maxval == b'255'
    assert len(set(pixels)) > 10, 'Framebuffer appears empty'
    def chunk(kind, data):
        return struct.pack('>I',len(data))+kind+data+struct.pack('>I',zlib.crc32(kind+data))
    scan = b''.join(b'\0'+pixels[y*width*3:(y+1)*width*3] for y in range(height))
    png = b'\x89PNG\r\n\x1a\n'+chunk(b'IHDR',struct.pack('>IIBBBBB',width,height,8,2,0,0,0))
    png += chunk(b'IDAT',zlib.compress(scan))+chunk(b'IEND',b'')
    (root / 'build/desktop.png').write_bytes(png)
    print('PASS: UEFI entry, ExitBootServices, desktop and nonempty framebuffer')
finally:
    if p.poll() is None:
        p.terminate()
    p.wait(timeout=10)
