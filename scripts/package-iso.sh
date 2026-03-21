#!/usr/bin/env bash
set -euo pipefail

: "${BUILDROOT_DIR:=/opt/buildroot}"
: "${OUTPUT_DIR:=$(pwd)/out}"

mkdir -p "$OUTPUT_DIR"
cat <<MSG
Packaging steps:
1. Build Buildroot with BR2_EXTERNAL=$(pwd)/buildroot
2. Copy grub.cfg and splash assets into the ISO staging tree
3. Emit bzImage + rootfs.cpio into $OUTPUT_DIR
4. Run grub-mkrescue -o $OUTPUT_DIR/newos.iso iso/
MSG
