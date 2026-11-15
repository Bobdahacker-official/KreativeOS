# Verification

The build workflow performs a release compilation and one QEMU boot smoke check. The check waits for serial markers after ExitBootServices and desktop initialization, then captures the framebuffer and confirms that it contains rendered pixel variation.

This check does not establish hardware compatibility beyond the configured virtual machine, or verify network interoperability, persistence under power loss, all input paths or every app. Those features are implemented in the source but require focused runtime verification before their behavior can be reported as tested.

Commands:

```sh
bash scripts/build.sh
python3 scripts/smoke.py
```

The boot image and EFI binary checksums are written to `build/SHA256SUMS`. CI preserves the serial log and screenshot alongside the disk images.
