#!/usr/bin/env python3
"""Boot the actual UEFI image and drive the graphical session through QMP."""
import argparse
from contextlib import contextmanager
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path
import re
import shutil
import socket
import subprocess
import tempfile
from threading import Thread
import time

ROOT = Path(__file__).resolve().parent.parent


class Monitor:
    def __init__(self, path):
        self.socket = socket.socket(socket.AF_UNIX)
        self.socket.settimeout(5)
        self.socket.connect(str(path))
        self.stream = self.socket.makefile("rwb")
        greeting = json.loads(self.stream.readline())
        assert "QMP" in greeting, greeting
        self.call("qmp_capabilities")

    def call(self, command, **arguments):
        self.stream.write(json.dumps({"execute": command, "arguments": arguments}).encode() + b"\n")
        self.stream.flush()
        while raw := self.stream.readline():
            result = json.loads(raw)
            if "return" in result:
                return result["return"]
            if "error" in result:
                raise RuntimeError(result["error"])
        raise RuntimeError(f"QEMU closed its monitor during {command}")

    def close(self):
        self.stream.close()
        self.socket.close()

    def pointer(self, x, y, button=None):
        def axis(name, position, size):
            return {"type": "abs", "data": {"axis": name, "value": round(position * 32767 / (size - 1))}}
        event = [axis("x", x, self.width), axis("y", y, self.height)]
        if button is not None:
            event.append({"type": "btn", "data": {"button": "left", "down": button}})
        self.call("input-send-event", events=event)
        time.sleep(0.12)

    def click(self, x, y):
        self.pointer(x, y, True)
        self.pointer(x, y, False)

    def write(self, string):
        mapping = {" ": "spc", "\n": "ret", "-": "minus", ".": "dot", "/": "slash",
                   ":": "semicolon", ";": "semicolon", "\t": "tab", "&": "7", ">": "dot",
                   "$": "4", "?": "slash", "=": "equal", "_": "minus", "'": "apostrophe"}
        for letter in string:
            code = mapping.get(letter, letter.lower())
            keys = ["shift", code] if letter in ">&$?:_" or letter.isupper() else [code]
            self.call("send-key", keys=[{"type": "qcode", "data": key} for key in keys], **{"hold-time": 30})
            time.sleep(0.072)


def wait_stage(path, process, stage, occurrence=1):
    for _ in range(350):
        log = path.read_text(errors="replace") if path.exists() else ""
        matches = re.findall(rf"KreativeOS {stage} \((\d+)x(\d+)\)", log)
        if len(matches) >= occurrence:
            if "KreativeOS: persistent storage mounted" not in log:
                raise RuntimeError(log + "\nExpected persistent ext4")
            return tuple(map(int, matches[-1]))
        if process.poll() is not None or "Kernel panic" in log:
            break
        time.sleep(0.15)
    raise RuntimeError(f"Graphical {stage} failed\n" + log)


class ProbeHandler(BaseHTTPRequestHandler):
    def log_message(self, *_args):
        pass

    def do_GET(self):
        body = b"kreativeos-network-ok\n"
        self.send_response(200)
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)


@contextmanager
def probe():
    with ThreadingHTTPServer(("127.0.0.1", 0), ProbeHandler) as server:
        thread = Thread(target=server.serve_forever, daemon=True)
        thread.start()
        try:
            yield server.server_address[1]
        finally:
            server.shutdown()
            thread.join()


def disk_file(image, name):
    result = subprocess.run(["debugfs", "-R", f"cat /{name}", image], capture_output=True, check=True)
    return result.stdout.decode()


def exercise(architecture, qemu_data=None, captures=None):
    tag = {"amd64": "x86_64", "arm64": "aarch64"}[architecture]
    env = os.environ.copy()
    with tempfile.TemporaryDirectory(prefix="kos-qemu-") as location, probe() as port:
        tmp = Path(location)
        image = tmp / "guest.img"
        shutil.copyfile(ROOT / "build" / architecture / f"KreativeOS-{tag}.img", image)
        serial = tmp / "serial.log"
        monitor = tmp / "qmp.sock"
        env.update(KOS_IMAGE=str(image), KOS_SERIAL=f"file:{serial}",
                   KOS_VM_VARS=str(tmp / "uefi.fd"),
                   KOS_RESOLUTION="800x600" if architecture == "amd64" else "480x800")
        options = ["-display", "none", "-qmp", f"unix:{monitor},server=on,wait=off"]
        if qemu_data:
            options.extend(["-L", qemu_data])
        process = subprocess.Popen(["bash", "scripts/run.sh", architecture, *options], cwd=ROOT, env=env)
        qmp = None
        try:
            w, h = wait_stage(serial, process, "greeter")
            qmp = Monitor(monitor)
            qmp.width, qmp.height = w, h
            if captures:
                time.sleep(0.8)
                captures.mkdir(parents=True, exist_ok=True)
                qmp.call("screendump", filename=str((captures / f"setup-{architecture}.ppm").resolve()))
            qmp.write("tester\nenglish8\nenglish8\n")
            wait_stage(serial, process, "ready")
            if captures:
                time.sleep(0.8)
                qmp.call("screendump", filename=str((captures / f"home-{architecture}.ppm").resolve()))
            nav = min(w - 14, 440)
            dock_x = (w - nav) // 2
            dock_step = nav // 3
            qmp.click(dock_x + dock_step * 3 // 2, h - 28)
            qmp.click(w - (16 if w < 500 else 28) - 43, 28)
            qmp.write("bootnote\nhello from kreativeos\n")
            qmp.click(w - (16 if w < 500 else 28) - 43, 28)
            if captures:
                qmp.call("screendump", filename=str((captures / f"notes-{architecture}.ppm").resolve()))
            qmp.click(dock_x + dock_step * 5 // 2, h - 28)
            time.sleep(0.65)
            qmp.write("uname -m > machine.txt\n")
            qmp.write("id -u > uid.txt\n")
            qmp.write("test -t 0 && test -t 1 && echo tty > pty.txt\n")
            qmp.write(f"wget -O link.txt http://10.0.2.2:{port}/probe\n")
            time.sleep(0.5)
            qmp.write("sync\n")
            if captures:
                qmp.call("screendump", filename=str((captures / f"terminal-{architecture}.ppm").resolve()))

            qmp.click(dock_x + dock_step // 2, h - 28)
            qmp.pointer(w - 48, 334, True)
            qmp.pointer(42, 334, True)
            qmp.pointer(42, 334, False)
            time.sleep(0.7)
            # Page two contains Clock, Network and Settings.
            cols = 4 if w >= 610 else 2
            margin = 16 if w < 500 else 28
            width = w - margin * 2
            step = (width - (cols - 1) * 10) // cols
            hero = 150 if h < 610 else 194
            settings_x = margin + (2 % cols) * (step + 10) + step // 2
            settings_y = 55 + 27 + hero + (2 // cols) * 106 + 48
            qmp.click(settings_x, settings_y)
            qmp.click(w // 2, 205)
            wait_stage(serial, process, "greeter", 2)
            qmp.write("tester\nwrongpass\n")
            time.sleep(1.7)
            assert serial.read_text(errors="replace").count("KreativeOS ready") == 1, "Bad password signed in"
            qmp.write("english8\n")
            wait_stage(serial, process, "ready", 2)
            if architecture == "amd64":
                qmp.pointer(w - 48, 334, True)
                qmp.pointer(42, 334, True)
                qmp.pointer(42, 334, False)
                time.sleep(0.7)
                qmp.click(settings_x, settings_y)
                qmp.click(w // 2, 205)
                wait_stage(serial, process, "greeter", 3)
                qmp.click(w - margin - 45, 27)
                qmp.write("guest\nguest789\nguest789\nenglish8\n")
                wait_stage(serial, process, "ready", 3)
                qmp.click(dock_x + dock_step * 5 // 2, h - 28)
                time.sleep(0.65)
                qmp.write("test -r /data/home/tester/bootnote.txt; echo $? > isolation.txt\n")
                qmp.write("id -u > uid.txt\n")
                qmp.write("sync\n")
            time.sleep(0.4)
            try:
                qmp.call("quit")
            except RuntimeError:
                pass
            process.wait(10)
        finally:
            if qmp:
                qmp.close()
            if process.poll() is None:
                process.terminate()
                process.wait(10)
        partition = tmp / "data.img"
        with image.open("rb") as disk, partition.open("wb") as data:
            disk.seek(81 * 1024 * 1024)
            for _ in range(174):
                data.write(disk.read(1024 * 1024))
        checked = subprocess.run(["e2fsck", "-p", partition], capture_output=True, text=True)
        assert checked.returncode in (0, 1), checked.stdout + checked.stderr
        assert disk_file(partition, "home/tester/bootnote.txt") == "hello from kreativeos\n", "Notes did not persist"
        machine = disk_file(partition, "home/tester/machine.txt").strip()
        assert machine == tag, f"Terminal returned architecture {machine!r}, expected {tag!r}"
        assert disk_file(partition, "home/tester/uid.txt").strip() == "1000", "Terminal did not use the owner's UID"
        assert disk_file(partition, "home/tester/pty.txt").strip() == "tty", "Shell did not use a PTY"
        assert disk_file(partition, "home/tester/link.txt") == "kreativeos-network-ok\n", "Ethernet transfer failed"
        assert "tester:1000:$y$" in disk_file(partition, "system/accounts"), "Owner hash is missing"
        if architecture == "amd64":
            assert disk_file(partition, "home/guest/isolation.txt").strip() == "1", "Another account could read the owner's home"
            assert disk_file(partition, "home/guest/uid.txt").strip() == "1001", "Second account UID is incorrect"
            assert "guest:1001:$y$" in disk_file(partition, "system/accounts"), "Guest hash is missing"
        print(f"PASS: {tag} UEFI, {w}x{h} GUI, login, private Notes, interactive PTY and Ethernet DHCP/HTTP")



if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("architecture", choices=["amd64", "arm64"])
    parser.add_argument("--qemu-data")
    parser.add_argument("--capture", type=Path)
    args = parser.parse_args()
    exercise(args.architecture, args.qemu_data, args.capture)
