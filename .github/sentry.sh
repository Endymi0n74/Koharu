#!/usr/bin/env bash
set -euo pipefail

# Upload symbols before stripping; Tauri bundles and signs afterward.
# Symbol upload needs both variables; without them the fork still strips but
# skips Sentry, which has no credentials configured here.
upload=0
if [ -n "${SENTRY_AUTH_TOKEN:-}" ] && [ -n "${SENTRY_ORG:-}" ]; then
  upload=1
else
  echo "sentry.sh: SENTRY_AUTH_TOKEN or SENTRY_ORG is not set, skipping symbol upload"
fi

binary=target/release/koharu
case "$RUNNER_OS" in
  macOS)
    binary=target/aarch64-apple-darwin/release/koharu
    dsymutil "$binary"
    if [ "$upload" = 1 ]; then
      sentry-cli debug-files upload --include-sources --wait "$binary.dSYM"
    fi
    strip -S -x "$binary"
    ;;
  Windows)
    if [ "$upload" = 1 ]; then
      sentry-cli debug-files upload --include-sources --wait "$binary.exe" "$binary.pdb"
    fi
    # Debug information is separate in the PDB, which Tauri does not bundle.
    ;;
  Linux)
    # Upload the full ELF so Sentry also has the unwind information.
    if [ "$upload" = 1 ]; then
      sentry-cli debug-files upload --include-sources --wait "$binary"
    fi
    llvm-strip --strip-debug "$binary"
    ;;
esac
