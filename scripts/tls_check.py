#!/usr/bin/env python3
"""Headless browser transport, certificate, media and ESP-AT integration check."""
from pathlib import Path
import subprocess, ssl, threading, http.server, os, time, wave, io, math, struct, gzip, sys, shutil
import json, socket
from esp_at_fixture import EspAtFixture
ROOT=Path(__file__).resolve().parent.parent
TLS=ROOT/'build/tls-test'
TLS.mkdir(parents=True,exist_ok=True)
def run(*args): subprocess.run(args,cwd=TLS,check=True,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
def ca(name):
 run('openssl','ecparam','-name','prime256v1','-genkey','-noout','-out',name+'.key')
 run('openssl','req','-new','-x509','-key',name+'.key','-out',name+'.pem','-days','2','-subj','/CN=KreativeOS local TLS check','-addext','basicConstraints=critical,CA:TRUE','-addext','keyUsage=critical,keyCertSign,cRLSign')
def leaf(name,issuer,ip):
 run('openssl','ecparam','-name','prime256v1','-genkey','-noout','-out',name+'.key')
 run('openssl','req','-new','-key',name+'.key','-out',name+'.csr','-subj','/CN='+ip)
 (TLS/(name+'.ext')).write_text(f'basicConstraints=critical,CA:FALSE\nkeyUsage=critical,digitalSignature\nextendedKeyUsage=serverAuth\nsubjectAltName=IP:{ip}\n')
 run('openssl','x509','-req','-in',name+'.csr','-CA',issuer+'.pem','-CAkey',issuer+'.key','-CAcreateserial','-out',name+'.pem','-days','1','-extfile',name+'.ext')
ca('ca');ca('other');leaf('trusted','ca','10.0.2.2');leaf('wrong-host','ca','10.0.2.99');leaf('untrusted','other','10.0.2.2')
run('openssl','x509','-in','ca.pem','-outform','DER','-out','ca.der')
# The test root is included only in this feature build, never the distributed kernel.
subprocess.run(['cargo','build','--release','--target','x86_64-unknown-uefi','--features','tls-smoke'],cwd=ROOT,check=True)
out=ROOT/'build/tls-smoke/esp/EFI/BOOT';out.mkdir(parents=True,exist_ok=True)
(out/'BOOTX64.EFI').write_bytes((ROOT/'target/x86_64-unknown-uefi/release/kreativeos.efi').read_bytes())
env=dict(os.environ,KREATIVE_BUILD_DIR='build/tls-smoke')
subprocess.run([sys.executable,'scripts/image.py'],cwd=ROOT,env=env,check=True)
requests=[]
clip = io.BytesIO()
with wave.open(clip, 'wb') as writer:
 writer.setparams((1,2,16000,0,'NONE','not compressed'))
 writer.writeframes(b''.join(struct.pack('<h',int(12000*math.sin(i*2*math.pi*440/16000))) for i in range(8000)))
class Handler(http.server.BaseHTTPRequestHandler):
 protocol_version = 'HTTP/1.1'
 def do_GET(self):
  if self.server.server_port == 18080:
   requests.append((18080, self.request_version, self.path))
   if self.headers.get('Host') != '10.0.2.2:18080':
    self.send_error(400)
    return
   self.send_response(308)
   self.send_header('Location', 'https://10.0.2.2:8443/?view=1')
   self.send_header('Content-Length', '0')
   self.end_headers()
   return
  requests.append((self.server.server_port,self.connection.version(),self.path))
  if self.path == '/image.png': data, kind=(ROOT/'assets/estuary.png').read_bytes(),'image/png'
  elif self.path == '/sound.wav': data, kind=clip.getvalue(),'audio/wav'
  else:
   if 'gzip' not in self.headers.get('Accept-Encoding', ''):
    self.send_error(406)
    return
   data = gzip.compress(b'<h1>KreativeOS HTTPS verified</h1><img src="/image.png"><audio src="/sound.wav"></audio><a href="?view=2&amp;mode=read">Read</a>')
   self.send_response(200)
   self.send_header('Content-Type', 'text/html')
   self.send_header('Content-Encoding', 'gzip')
   self.send_header('Transfer-Encoding', 'chunked')
   self.end_headers()
   for chunk in (data[:17], data[17:]):
    self.wfile.write(f'{len(chunk):x}\r\n'.encode() + chunk + b'\r\n')
   self.wfile.write(b'0\r\n\r\n')
   return
  self.send_response(200);self.send_header('Content-Type',kind);self.send_header('Content-Length',str(len(data)));self.end_headers();self.wfile.write(data)
 def log_message(self,*args): pass
servers=[http.server.ThreadingHTTPServer(('127.0.0.1',18080),Handler)]
threading.Thread(target=servers[0].serve_forever,daemon=True).start()
for port,name in [(8443,'trusted'),(8444,'wrong-host'),(8445,'untrusted')]:
 server=http.server.ThreadingHTTPServer(('127.0.0.1',port),Handler)
 ctx=ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER);ctx.minimum_version=ssl.TLSVersion.TLSv1_3;ctx.maximum_version=ssl.TLSVersion.TLSv1_3
 ctx.load_cert_chain(TLS/(name+'.pem'),TLS/(name+'.key'))
 server.socket=ctx.wrap_socket(server.socket,server_side=True)
 threading.Thread(target=server.serve_forever,daemon=True).start();servers.append(server)
esp=EspAtFixture()
recording=ROOT/'build/https-playback.wav'
recording.unlink(missing_ok=True)
env=dict(os.environ,BOOT_IMAGE='build/tls-smoke/KreativeOS.img',SERIAL_LOG='build/tls-check.log',
         AUDIO_SPEC=f'driver=wav,path={recording}',ESP_AT_SERIAL=f'tcp:127.0.0.1:{esp.port}')

(ROOT/'build/tls-check.log').unlink(missing_ok=True)
with socket.socket() as address:
 address.bind(('127.0.0.1',0))
 monitor = address.getsockname()[1]
options = ['-display','none','-no-reboot','-snapshot','-qmp',f'tcp:127.0.0.1:{monitor},server=on,wait=off']
if os.name == 'nt':
 qemu = Path(os.environ.get('KREATIVE_QEMU', Path(os.environ['ProgramFiles'])/'qemu/qemu-system-x86_64.exe'))
 firmware = qemu.parent/'share'
 code = Path(os.environ.get('OVMF_CODE', firmware/'edk2-x86_64-code.fd'))
 template = Path(os.environ.get('OVMF_VARS', firmware/'edk2-i386-vars.fd'))
 variables = ROOT/'build/tls-smoke/OVMF_VARS.fd'
 shutil.copyfile(template, variables)
 launcher = [
  str(qemu), '-machine','pc','-cpu','max','-m','256',
  '-drive',f'if=pflash,format=raw,readonly=on,file={code}',
  '-drive',f'if=pflash,format=raw,file={variables}',
  '-drive',f"if=none,id=kos,format=raw,file={env['BOOT_IMAGE']}",
  '-device','virtio-blk-pci,drive=kos,bootindex=1',
  '-device',f"{env.get('NIC_MODEL', 'e1000e')},netdev=net0",'-netdev','user,id=net0',
  '-vga','std','-serial',f"file:{env['SERIAL_LOG']}",'-serial',env['ESP_AT_SERIAL'],
  '-audiodev',env['AUDIO_SPEC']+',id=kos_audio','-device','AC97,audiodev=kos_audio',
 ]
else:
 launcher = ['bash','scripts/run.sh']
p=subprocess.Popen(launcher+options,cwd=ROOT,env=env)
try:
 deadline=time.monotonic()+90
 while time.monotonic()<deadline:
  log=(ROOT/'build/tls-check.log').read_text(errors='replace') if (ROOT/'build/tls-check.log').exists() else ''
  if 'TLS CHECK FAILED' in log: raise RuntimeError(log)
  if 'ESP-AT HTTPS PASS' in log: break
  if p.poll() is not None: raise RuntimeError('QEMU exited: '+log)
  time.sleep(.2)
 else: raise RuntimeError('TLS check timed out: '+log)
 assert log.count('HTTPS rejection PASS')==2 and 'HTTPS media PASS' in log,log
 assert 'HTTP redirect, HTTPS gzip and page link PASS' in log,log
 assert requests==[(18080,'HTTP/1.1','/start?old=1')]+[(8443,'TLSv1.3',path) for path in ('/?view=1','/image.png','/sound.wav','/')],requests
 time.sleep(1)
 with socket.create_connection(('127.0.0.1',monitor),timeout=5) as client:
  with client.makefile('rb') as replies:
   assert 'QMP' in json.loads(replies.readline()), 'Missing QMP greeting'
   client.sendall(b'{"execute":"qmp_capabilities"}\n')
   assert 'return' in json.loads(replies.readline()), 'QMP setup failed'
   client.sendall(b'{"execute":"quit"}\n')
   assert p.wait(timeout=10) == 0, 'QEMU failed to shut down cleanly'
 with wave.open(str(recording),'rb') as recorded:
  assert any(recorded.readframes(recorded.getnframes())), 'AC97 output was silent'
 print('PASS: HTTP upgrade, HTTPS chunked gzip, page link, certificate rejection, PNG, WAV, and ESP-AT TLS.')
finally:
 if p.poll() is None: p.terminate();p.wait(timeout=10)
 esp.listener.close()
 for server in servers: server.shutdown();server.server_close()
