# Verification

The build workflow performs a release compilation and one QEMU boot smoke check. The check waits for serial markers after ExitBootServices and desktop initialization, then captures the framebuffer and confirms that it contains rendered pixel variation.

This check does not establish hardware compatibility beyond the configured virtual machine, or verify network interoperability, persistence under power loss, all input paths or every app. Those features are implemented in the source but require focused runtime verification before their behavior can be reported as tested.

Commands:

```sh
bash scripts/build.sh
python3 scripts/smoke.py
```

The boot image and EFI binary checksums are written to `build/SHA256SUMS`. CI preserves the serial log and screenshot alongside the disk images.

## Recorded build

The release compiled successfully with Rust 1.100.0 for `x86_64-unknown-uefi`. The QEMU boot check passed on 15 November 2026, including ExitBootServices, PS/2 initialization, KFS mount, RTL8139 initialization and desktop rendering. The screenshot was visually inspected.

Build: https://github.com/Bobdahacker-official/KreativeOS/actions/runs/43672298346

The committed formatted Rust source is the exact source produced by rustfmt and compiled in that run. The distribution bundle is the unmodified build artifact (SHA-256 listed in `dist/SHA256SUMS`).
