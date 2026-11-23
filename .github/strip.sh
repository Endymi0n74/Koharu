#!/usr/bin/env bash
set -euo pipefail

# The release ships binaries without debug information. This fork has no Sentry
# credentials, so nothing collects that information and there is no symbol
# bundle to build or upload: this only strips.
case "$RUNNER_OS" in
  macOS)
    # `dsymutil` built the bundle Sentry was given; without it, strip alone.
    strip -S -x target/aarch64-apple-darwin/release/koharu
    ;;
  Linux)
    # The full ELF carried unwind information for Sentry's symbolication.
    llvm-strip --strip-debug target/release/koharu
    ;;
  Windows)
    # Debug information is separate in the PDB, which Tauri does not bundle.
    ;;
esac
