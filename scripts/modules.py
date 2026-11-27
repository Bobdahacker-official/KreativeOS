#!/usr/bin/env python3
"""Collect configured boot drivers and their module dependencies."""
import lzma
from pathlib import Path
import re
import shutil
import subprocess
import sys

source, target = map(Path, sys.argv[1:3])
target.mkdir(parents=True, exist_ok=True)
modules = {}
for path in source.rglob("*.ko*"):
    name = re.split(r"\.ko(?:\..*)?$", path.name)[0].replace("-", "_")
    modules[name] = path

written = {}

def include(name):
    name = name.replace("-", "_")
    if name not in modules:
        return None
    if name in written:
        return written[name][0]
    path = modules[name]
    if path.suffix == ".zst":
        data = subprocess.check_output(["zstd", "-dc", path])
    elif path.suffix == ".xz":
        data = lzma.decompress(path.read_bytes())
    else:
        data = path.read_bytes()
    relative = str(path.relative_to(source)).split(".ko")[0] + ".ko"
    output = target / relative
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_bytes(data)
    written[name] = (relative, [])
    match = re.search(rb"\x00depends=([^\x00]*)\x00", data)
    dependencies = match.group(1).decode().split(",") if match else []
    written[name][1].extend(dep for item in dependencies if (dep := include(item)))
    return relative

init = Path(sys.argv[3]).read_text()
block = init.split("for module in ", 1)[1].split("; do", 1)[0]
for driver in block.replace("\\", " ").split():
    include(driver)

for f in ("modules.builtin", "modules.builtin.modinfo", "modules.order"):
    if (source / f).exists():
        shutil.copyfile(source / f, target / f)

subprocess.run(["depmod", "-b", str(target.parents[2]), target.name], check=True)
