#!/usr/bin/env python3
"""One local TLS integration run: trusted server, wrong host, untrusted issuer."""
from pathlib import Path
import subprocess, ssl, threading, http.server, os, time
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
subprocess.run(['python3','scripts/image.py'],cwd=ROOT,env=env,check=True)
requests=[]
class Handler(http.server.BaseHTTPRequestHandler):
 def do_GET(self):
  requests.append((self.server.server_port,self.connection.version()))
  data=b'<h1>KreativeOS HTTPS verified</h1><p>Authenticated TLS 1.3 response.</p>'
  self.send_response(200);self.send_header('Content-Length',str(len(data)));self.end_headers();self.wfile.write(data)
 def log_message(self,*args): pass
servers=[]
for port,name in [(8443,'trusted'),(8444,'wrong-host'),(8445,'untrusted')]:
 server=http.server.ThreadingHTTPServer(('0.0.0.0',port),Handler)
 ctx=ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER);ctx.minimum_version=ssl.TLSVersion.TLSv1_3;ctx.maximum_version=ssl.TLSVersion.TLSv1_3
 ctx.load_cert_chain(TLS/(name+'.pem'),TLS/(name+'.key'))
 server.socket=ctx.wrap_socket(server.socket,server_side=True)
 threading.Thread(target=server.serve_forever,daemon=True).start();servers.append(server)
env=dict(os.environ,BOOT_IMAGE='build/tls-smoke/KreativeOS.img',SERIAL_LOG='build/tls-check.log')
(ROOT/'build/tls-check.log').unlink(missing_ok=True)
p=subprocess.Popen(['bash','scripts/run.sh','-display','none','-no-reboot'],cwd=ROOT,env=env)
try:
 deadline=time.monotonic()+90
 while time.monotonic()<deadline:
  log=(ROOT/'build/tls-check.log').read_text(errors='replace') if (ROOT/'build/tls-check.log').exists() else ''
  if 'TLS CHECK FAILED' in log: raise RuntimeError(log)
  if log.count('HTTPS rejection PASS')==2 and 'HTTPS positive PASS' in log: break
  if p.poll() is not None: raise RuntimeError('QEMU exited: '+log)
  time.sleep(.2)
 else: raise RuntimeError('TLS check timed out: '+log)
 assert requests==[(8443,'TLSv1.3')],requests
 print('PASS: verified TLS 1.3 GET; wrong hostname rejected; untrusted issuer rejected.')
finally:
 p.terminate();p.wait(timeout=10)
 for server in servers: server.shutdown();server.server_close()
