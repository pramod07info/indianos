#!/usr/bin/env bash
# Mandatory native QEMU/OVMF provisioning without Canonical apt DNS.
# Binaries come from Nix's signed binary cache and a pinned nixpkgs revision.
# This is NOT Docker's user-mode binfmt emulator. Tests boot real x86-64 guests.
set -euo pipefail

NIXPKGS="github:NixOS/nixpkgs/ac588ef1c914865dad517b337ea0eb9694a4021c"

firmware_exists() {
  local firmware
  for firmware in \
      /usr/share/OVMF/OVMF_CODE_4M.fd \
      /usr/share/OVMF/OVMF_CODE.fd \
      /usr/share/edk2/ovmf/OVMF_CODE_4M.fd \
      /usr/share/edk2/ovmf/OVMF_CODE.fd; do
    [[ -s "$firmware" ]] && return 0
  done
  return 1
}

have_tools() {
  local tool
  for tool in clang ld.lld xorriso qemu-system-x86_64 curl openssl; do
    if ! command -v "$tool" >/dev/null 2>&1; then
      echo "::warning::IndianOS CI dependency missing: $tool"
      return 1
    fi
  done
  firmware_exists
}

nix_package() {
  # --no-link avoids checkout mutation. Nix checks signed substituter paths
  # against the cache's configured trusted public key.
  nix build --no-link --print-out-paths "$NIXPKGS#$1"
}

nix_output_binary() {
  local outputs="$1" binary="$2" root
  while IFS= read -r root; do
    [[ -n "$root" ]] || continue
    if [[ -x "$root/bin/$binary" ]]; then
      printf '%s\n' "$root/bin/$binary"
      return 0
    fi
  done <<< "$outputs"
  return 1
}

nix_output_firmware() {
  local outputs="$1" root
  while IFS= read -r root; do
    [[ -n "$root" ]] || continue
    if [[ -s "$root/FV/OVMF_CODE.fd" ]]; then
      printf '%s\n' "$root/FV/OVMF_CODE.fd"
      return 0
    fi
  done <<< "$outputs"
  return 1
}

install_from_nix() {
  if ! command -v nix >/dev/null 2>&1; then
    echo "::error::Nix is required for offline-from-Ubuntu QEMU provisioning"
    return 1
  fi

  local qemu_outputs xorriso_outputs ovmf_outputs qemu_binary xorriso_binary firmware bin_dir
  echo "IndianOS CI: obtaining QEMU, xorriso and OVMF from the Nix binary cache"
  qemu_outputs="$(nix_package qemu)"
  xorriso_outputs="$(nix_package xorriso)"
  ovmf_outputs="$(nix_package OVMF.fd)"
  # Nix may return several outputs (out/lib/dev/man). Find the executable
  # among immutable store paths, not as one multi-line filename.
  qemu_binary="$(nix_output_binary "$qemu_outputs" qemu-system-x86_64)" || {
    echo "::error::Nix QEMU is missing its system x86-64 emulator"; return 1;
  }
  xorriso_binary="$(nix_output_binary "$xorriso_outputs" xorriso)" || {
    echo "::error::Nix xorriso image builder is missing"; return 1;
  }
  firmware="$(nix_output_firmware "$ovmf_outputs")" || {
    echo "::error::Nix OVMF output has no real UEFI firmware"; return 1;
  }

  bin_dir="$HOME/.local/bin"
  mkdir -p "$bin_dir"
  ln -sfn "$qemu_binary" "$bin_dir/qemu-system-x86_64"
  ln -sfn "$xorriso_binary" "$bin_dir/xorriso"
  export PATH="$bin_dir:$PATH"
  # Ubuntu's runner may not provide a generic clang/ld.lld symlink even
  # when versioned LLVM is installed. Provision these from the same pinned
  # Nix cache rather than falling back to broken Canonical mirrors.
  local llvm_tool llvm_attr llvm_outputs llvm_binary
  for llvm_tool in clang ld.lld; do
    if command -v "$llvm_tool" >/dev/null 2>&1; then
      continue
    fi
    if [[ "$llvm_tool" == clang ]]; then llvm_attr=clang; else llvm_attr=lld; fi
    llvm_outputs="$(nix_package "$llvm_attr")"
    llvm_binary="$(nix_output_binary "$llvm_outputs" "$llvm_tool")" || {
      echo "::error::Nix is missing mandatory LLVM command $llvm_tool"; return 1;
    }
    ln -sfn "$llvm_binary" "$bin_dir/$llvm_tool"
  done
  # Existing jobs and Makefile probe the conventional Ubuntu OVMF pathname.
  # Only a symlink is created; firmware remains the read-only Nix store object.
  sudo mkdir -p /usr/share/OVMF
  sudo ln -sfn "$firmware" /usr/share/OVMF/OVMF_CODE.fd
  if [[ -v GITHUB_PATH ]]; then
    printf '%s\n' "$bin_dir" >> "$GITHUB_PATH"
  fi
}

if ! have_tools; then
  install_from_nix
fi

if ! have_tools; then
  echo "::error::Mandatory QEMU, OVMF firmware or compiler tool is unavailable"
  exit 1
fi
# A successful installation is not a QEMU acceptance pass. CI must still boot
# the real guests and produce exact-head artifacts in each existing workflow.
qemu-system-x86_64 --version >/dev/null
xorriso -version >/dev/null
clang --version >/dev/null
ld.lld --version >/dev/null
echo "IndianOS CI: QEMU system emulator, OVMF, ISO builder and linker verified"
