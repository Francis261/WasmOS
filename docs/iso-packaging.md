# ISO packaging workflow

1. Install Buildroot and configure an x86_64 target with GRUB, X.Org, Chromium, and Rust runtime dependencies.
2. Mount `buildroot/board/newos/rootfs-overlay` as the root filesystem overlay.
3. Copy the `webos/` directory into `/opt/newos/` in the overlay and install the `wasmos-host` binary into `/usr/bin/`.
4. Place `buildroot/board/newos/grub/grub.cfg` and a `newos-splash.png` asset into the ISO staging tree under `/boot/grub/`.
5. Generate kernel (`bzImage`) and initramfs (`rootfs.cpio`), then call `grub-mkrescue -o out/newos.iso iso/`.
6. Test the ISO with `qemu-system-x86_64 -m 2048 -cdrom out/newos.iso -boot d -enable-kvm`.
