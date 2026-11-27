#!/usr/bin/env python3
"""Boot the actual UEFI image and drive the graphical session through QMP."""
import argparse
import json
import os
from pathlib import Path
import re
import shutil
import socket
import subprocess
import tempfile
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
        mapping = {" ": "spc", "\n": "ret", "-": "minus", ".": "dot", "/": "slash"}
        for letter in string:
            keys = ["shift", "dot"] if letter == ">" else [mapping.get(letter, letter)]
            self.call("send-key", keys=[{"type": "qcode", "data": key} for key in keys], **{"hold-time": 25})
            time.sleep(0.055)


def wait_ready(path, process):
    for _ in range(350):
        log = path.read_text(errors="replace") if path.exists() else ""
        if match := re.search(r"KreativeOS ready \((\d+)x(\d+)\)", log):
            if "KreativeOS: persistent storage mounted" not in log:
                raise RuntimeError(log + "\nExpected persistent ext4")
            return tuple(map(int, match.groups()))
        if process.poll() is not None or "Kernel panic" in log:
            break
        time.sleep(0.15)
    raise RuntimeError("Graphical boot failed\n" + log)


def disk_file(image, name):
    result = subprocess.run(["debugfs", "-R", f"cat /{name}", image], capture_output=True, check=True)
    return result.stdout.decode()


def exercise(architecture, qemu_data=None, captures=None):
    tag = {"amd64": "x86_64", "arm64": "aarch64"}[architecture]
    env = os.environ.copy()
    with tempfile.TemporaryDirectory(prefix="kos-qemu-") as location:
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
            w, h = wait_ready(serial, process)
            qmp = Monitor(monitor)
            qmp.width, qmp.height = w, h
            if captures:
                time.sleep(0.8)
                captures.mkdir(parents=True, exist_ok=True)
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
            qmp.write("uname -m > machine.txt\n")
            time.sleep(0.35)
            qmp.write("sync\n")
            time.sleep(0.35)
            if captures:
                qmp.call("screendump", filename=str((captures / f"terminal-{architecture}.ppm").resolve()))
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
        assert disk_file(partition, "bootnote.txt") == "hello from kreativeos\n", "Notes did not persist"
        assert disk_file(partition, "machine.txt").strip() == tag, "Terminal did not write the guest architecture"
        print(f"PASS: {tag} UEFI, {w}x{h} GUI, Notes saved to ext4, shell executed uname")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("architecture", choices=["amd64", "arm64"])
    parser.add_argument("--qemu-data")
    parser.add_argument("--capture", type=Path)
    args = parser.parse_args()
    exercise(args.architecture, args.qemu_data, args.capture)
