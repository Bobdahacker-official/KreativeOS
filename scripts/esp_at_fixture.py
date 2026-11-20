"""Serial fixture for the ESP-AT commands KreativeOS uses. Proxies a local TCP peer."""
import re
import socket
import threading


class EspAtFixture:
    def __init__(self):
        self.listener = socket.socket()
        self.listener.bind(("127.0.0.1", 0))
        self.listener.listen(1)
        self.port = self.listener.getsockname()[1]
        self.worker = threading.Thread(target=self.serve, daemon=True)
        self.worker.start()

    def serve(self):
        try:
            uart, _ = self.listener.accept()
            with uart, uart.makefile("rb") as serial:
                self.commands(uart, serial)
        except OSError:
            return

    def commands(self, uart, serial):
        send_lock = threading.Lock()
        state = threading.Lock()
        received = bytearray()
        peer = None
        closed = False

        def send(data):
            with send_lock:
                uart.sendall(data)

        def read_peer(conn):
            nonlocal closed
            try:
                while data := conn.recv(4096):
                    with state:
                        notify = not received
                        received.extend(data)
                        if notify:
                            send(f"\r\n+IPD,{len(received)}\r\n".encode())
            except OSError:
                pass
            finally:
                with state:
                    closed = True
                    if not received:
                        send(b"\r\nCLOSED\r\n")

        while line := serial.readline():
            if not line.startswith(b"AT"): continue
            line = line.decode("ascii", errors="strict").strip()
            if line == 'AT+CWJAP="KreativeOSTest","test-credentials"':
                send(b"\r\nWIFI CONNECTED\r\nWIFI GOT IP\r\nOK\r\n")
            elif match := re.fullmatch(r'AT\+CIPSTART="TCP","10\.0\.2\.2",(8443)', line):
                peer = socket.create_connection(("127.0.0.1", int(match[1])))
                send(b"\r\nCONNECT\r\nOK\r\n")
                threading.Thread(target=read_peer, args=(peer,), daemon=True).start()
            elif match := re.fullmatch(r"AT\+CIPSEND=(\d+)", line):
                send(b"\r\nOK\r\n>")
                binary = serial.read(int(match[1]))
                peer.sendall(binary)
                send(b"\r\nSEND OK\r\n")
            elif match := re.fullmatch(r"AT\+CIPRECVDATA=(\d+)", line):
                with state:
                    size = min(int(match[1]), len(received))
                    binary = bytes(received[:size])
                    del received[:size]
                    send(f"\r\n+CIPRECVDATA:{size},".encode() + binary + b"\r\nOK\r\n")
                    if received:
                        send(f"\r\n+IPD,{len(received)}\r\n".encode())
                    elif closed:
                        send(b"\r\nCLOSED\r\n")
            elif line == "AT+CIPCLOSE":
                if peer:
                    peer.close()
                send(b"\r\nOK\r\n")
            elif line in ("AT", "ATE0", "AT+SYSSTORE=0", "AT+CWMODE=1", "AT+CIPMUX=0", "AT+CIPRECVTYPE=1"):
                send(b"\r\nOK\r\n")
            else:
                send(b"\r\nERROR\r\n")
