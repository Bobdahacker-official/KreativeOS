#!/usr/bin/env python3
"""Stage WPA Supplicant, its ELF runtime, and selected Linux device firmware."""

from pathlib import Path
import re
import shutil
import subprocess
import sys

pool, stage, root = map(Path, sys.argv[1:])
runtime = stage / "network"
runtime.mkdir()
(root / "lib").mkdir(exist_ok=True)
for package in sorted(pool.glob("*.deb")):
    subprocess.run(["dpkg-deb", "-x", package, runtime], check=True)

firmware_patterns = (
    "intel/iwlwifi/iwlwifi-cc-a0-*", "intel/iwlwifi/iwlwifi-ty-a0-gf-a0-*",
    "iwlwifi-cc-a0-*", "iwlwifi-ty-a0-gf-a0-*",
    "brcm/brcmfmac43455-sdio*", "brcm/brcmfmac43430-sdio*",
    "rtl_nic/*", "rtlwifi/*", "rtl_bt/rtl8761*",
    "rtw88/*", "rtw89/rtw8852a*", "rtw89/rtw8852b*",
)
for origin in (runtime / "usr/lib/firmware", runtime / "lib/firmware"):
    if not origin.is_dir():
        continue
    for pattern in firmware_patterns:
        for path in sorted(origin.glob(pattern)):
            if path.is_file():
                target = root / "lib/firmware" / path.relative_to(origin)
                target.parent.mkdir(parents=True, exist_ok=True)
                shutil.copyfile(path, target)

for name in ("regulatory.db", "regulatory.db.p7s"):
    source = runtime / "usr/lib/firmware" / (name + "-debian")
    if source.is_file():
        target = root / "lib/firmware" / name
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(source, target)


def elf(filename):
    return subprocess.check_output(["readelf", "-d", filename], text=True)

libraries = {}
for base in (runtime / "usr/lib", runtime / "lib"):
    if base.is_dir():
        for path in base.rglob("*.so*"):
            if path.is_file():
                libraries.setdefault(path.name, path)

staged = set()
def install_library(name):
    if name in staged:
        return
    if name not in libraries:
        raise RuntimeError(f"Missing ELF runtime: {name}")
    staged.add(name)
    source = libraries[name]
    library = root / "lib" / name
    shutil.copyfile(source, library)
    if name.startswith("ld-"):
        library.chmod(0o755)
    stage_dependencies(source)

def stage_dependencies(source):
    for name in re.findall(r"Shared library: \[(.*?)\]", elf(source)):
        install_library(name)

def details(filename):
    return subprocess.check_output(["readelf", "-l", filename], text=True)

for executable in ("wpa_supplicant", "wpa_cli", "iw"):
    binary = runtime / "usr/sbin" / executable
    if not binary.is_file():
        raise RuntimeError(f"Missing networking program: {executable}")
    shutil.copyfile(binary, root / "sbin" / executable)
    (root / "sbin" / executable).chmod(0o755)
    interpreter = re.search(r"Requesting program interpreter: (.*?)\]", details(binary)).group(1)
    stage_dependencies(binary)
    loader = Path(interpreter).name
    install_library(loader)
    original = root / interpreter.lstrip("/")
    regular = root / "lib" / loader
    if original != regular and not original.is_symlink() and not original.exists():
        original.parent.mkdir(parents=True, exist_ok=True)
        original.symlink_to("/lib/" + loader)
