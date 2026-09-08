#!/bin/zsh
# Attach the APFS sparsebundle that holds PumpHound journals, artifacts, and
# Rust build caches. The bundle lives on the Crucial X9 (ExFAT); APFS is
# required for unix permissions and large directory trees.
#
# Safe to run from LaunchAgent: never detach, never re-attach an already
# attached image. Do not WatchPath the X9 root — every Docker.raw /
# sparsebundle band write would re-enter this script and can unmount the volume.
set -euo pipefail

BUNDLE="/Volumes/Crucial X9/PumpHoundData.sparsebundle"
MOUNT="/Volumes/PumpHoundData"
LOCKDIR="/tmp/pumphound-x9-mount.lock"

if ! mkdir "$LOCKDIR" 2>/dev/null; then
  exit 0
fi
trap 'rmdir "$LOCKDIR" 2>/dev/null || true' EXIT

if [[ -d $MOUNT/data && -d $MOUNT/artifacts && -d $MOUNT/rust-target ]]; then
  exit 0
fi

if [[ ! -d $BUNDLE ]]; then
  echo "X9 sparsebundle not present: $BUNDLE" >&2
  exit 0
fi

mount_named_volume() {
  local dev
  dev=$(diskutil list | awk '/PumpHoundData/{print $NF; exit}')
  if [[ -n ${dev:-} ]]; then
    diskutil mount "$dev" >/dev/null 2>&1 || true
  fi
}

if hdiutil info | grep -F -q "$BUNDLE"; then
  mount_named_volume
  exit 0
fi

hdiutil attach "$BUNDLE" -nobrowse >/dev/null
