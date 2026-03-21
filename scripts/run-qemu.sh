#!/usr/bin/env bash
set -euo pipefail
ISO_PATH="${1:?iso path required}"

qemu-system-x86_64 \
  -m 2048 \
  -smp 2 \
  -enable-kvm \
  -cdrom "$ISO_PATH" \
  -boot d \
  -serial mon:stdio \
  -display gtk
