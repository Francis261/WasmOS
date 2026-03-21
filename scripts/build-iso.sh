#!/usr/bin/env bash
set -euo pipefail

KERNEL_PATH="${1:?kernel path required}"
INITRD_PATH="${2:?initrd path required}"
GRUB_EFI_DIR="${3:?grub efi directory required}"
OUTPUT_ISO="${4:?output iso path required}"
ISO_ROOT="$(mktemp -d)"

mkdir -p "$ISO_ROOT/boot/grub" "$ISO_ROOT/EFI/BOOT"
cp "$KERNEL_PATH" "$ISO_ROOT/boot/vmlinuz"
cp "$INITRD_PATH" "$ISO_ROOT/boot/initrd.img"
cp boot/grub/grub.cfg "$ISO_ROOT/boot/grub/grub.cfg"
if [[ -f boot/grub/splash.png ]]; then
  cp boot/grub/splash.png "$ISO_ROOT/boot/grub/splash.png"
fi
cp -r "$GRUB_EFI_DIR"/* "$ISO_ROOT/EFI/BOOT/"

grub-mkrescue -o "$OUTPUT_ISO" "$ISO_ROOT"
