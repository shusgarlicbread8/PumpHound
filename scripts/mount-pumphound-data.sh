#!/usr/bin/env zsh
# Attach the APFS sparsebundle that holds PumpHound journals, artifacts, and
# Rust build caches. The bundle lives on the Crucial X9 (ExFAT); APFS is
# required for unix permissions and large directory trees.
set -euo pipefail
BUNDLE="/Volumes/Crucial X9/PumpHoundData.sparsebundle"
MOUNT="/Volumes/PumpHoundData"
if [[ ! -d $BUNDLE ]]; then
  echo "X9 sparsebundle not present: $BUNDLE" >&2
  exit 0
fi
if [[ -d $MOUNT/data ]]; then
  exit 0
fi
hdiutil attach "$BUNDLE" -mountpoint "$MOUNT" -nobrowse
