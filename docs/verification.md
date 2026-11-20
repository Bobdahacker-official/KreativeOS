# Verification

The release was built with Rust 1.100.0 for `x86_64-unknown-uefi` and booted in QEMU. The smoke check waits for the first desktop frame, then captures the framebuffer. The desktop screenshot in this repository came from that boot.

The HTTPS check runs the guest against three local TLS 1.3 servers. The browser fetched a page from the trusted server and rejected a certificate for the wrong host and a certificate from an untrusted issuer. Only the trusted server received an HTTP request. The check builds its own guest with a temporary test root. The distribution uses the bundled Mozilla roots.

```sh
bash scripts/build.sh
python3 scripts/smoke.py
python3 scripts/tls_check.py
```

The distribution contains the boot image, a blank data disk, the release EFI binary, the desktop screenshot and serial logs. Its SHA-256 is in `dist/SHA256SUMS`.

The desktop update also booted with RTL8139 and a second FAT disk on a QEMU xHCI USB controller. The gallery rendered the imported BMP, switched to the bundled PNG with the keyboard, and switched to the bundled JPEG with a PS/2 wheel event. The HTTPS integration check passed over the emulated Intel 82540EM.

The media build also moved the PS/2 pointer across three distinct framebuffer positions and checked that it left no trail. A local HTTPS server delivered an HTML image link and audio link. The guest decoded the PNG and played the WAV through AC'97, and the QEMU recording contained nonzero samples. The ESP-AT serial protocol fixture proxied a separate authenticated HTTPS session. Physical ESP32 association was not exercised in this run.
