# ISO packaging and boot testing

## 1. Build the minimal root filesystem

Use Buildroot or Ubuntu minimal to assemble a root filesystem containing:

- Linux kernel (`vmlinuz`)
- initrd or squashfs rootfs (`initrd.img`)
- GRUB EFI files
- Chromium
- Node.js runtime
- the contents of this repository copied into `/opt/wasmos`

For Buildroot, copy:

- `scripts/start-webos.sh` into `/usr/local/bin/start-webos.sh`
- `scripts/start-backend.sh` into `/usr/local/bin/start-backend.sh`
- `scripts/kiosk-chromium.sh` into `/usr/local/bin/kiosk-chromium.sh`
- `web/` into `/opt/wasmos/web`
- `server/` into `/opt/wasmos/server`

## 2. Create the ISO tree

Expected ISO tree:

```text
iso/
├── boot/
│   ├── grub/
│   │   ├── grub.cfg
│   │   └── splash.png
│   ├── initrd.img
│   └── vmlinuz
└── EFI/
    └── BOOT/
        └── BOOTX64.EFI
```

## 3. Package the ISO

Run:

```bash
bash scripts/build-iso.sh /path/to/kernel /path/to/initrd /path/to/grub-efi-directory out/wasmos.iso
```

## 4. Test with QEMU

Run:

```bash
bash scripts/run-qemu.sh out/wasmos.iso
```

Recommended QEMU settings in this repo target 2 GB RAM and UEFI boot.

## 5. Boot on hardware

1. Write the generated ISO to a USB drive.
2. Boot a UEFI-capable system from the USB drive.
3. GRUB auto-starts the kernel after a 0-second timeout.
4. The OS launches Chromium in kiosk mode into the local desktop.

## 6. App installation model

- Ship built-in apps under `/apps/<app-id>` in the image.
- Persist user-installed apps under `/data/apps-installed/<app-id>`.
- The desktop loader merges manifests from both locations at runtime.

## 7. Runtime bridge

The Node backend can spawn or proxy the Rust runtime binary so browser apps can request WASM execution indirectly without direct system access.
