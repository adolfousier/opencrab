#!/usr/bin/env bash
# Build unreleased dev binaries for community testing.
#
# CI is untouched: release.yml builds natively per platform on GitHub runners
# and never invokes `cross`. This exists so a dev drop can be produced without
# spending Actions minutes.
#
# Builds 4 of the 5 release targets. Windows (x86_64-pc-windows-msvc) is NOT
# buildable here: the MSVC linker does not exist on macOS. Ship the four and
# say so, or run CI for Windows alone.
#
# Linux targets go through `cross` (needs Docker/colima running) because macOS
# cannot link Linux binaries; Cross.toml installs libasound2-dev, which
# alsa-sys needs via rodio for local-stt/local-tts.
set -uo pipefail

# cross publishes its images for linux/amd64 ONLY, including the image for the
# aarch64 target. On an arm64 host Docker looks for an arm64 manifest, finds
# none, and fails both Linux targets with "no match for platform in manifest"
# before a single crate compiles. Asking for amd64 explicitly makes the host
# run them under emulation (Rosetta on Apple Virtualization.Framework).
export DOCKER_DEFAULT_PLATFORM=linux/amd64

cd "$(dirname "$0")/.."
VERSION="$(grep -m1 '^version' Cargo.toml | cut -d'"' -f2)"
STAMP="$(git rev-parse --short HEAD)"
OUT="dist/v${VERSION}-dev-${STAMP}"
mkdir -p "$OUT"

echo "OpenCrabs dev binaries - v${VERSION} @ ${STAMP}"
echo "Output: $OUT"
echo

TARGETS="aarch64-apple-darwin|cargo x86_64-apple-darwin|cargo x86_64-unknown-linux-gnu|cross aarch64-unknown-linux-gnu|cross"

FAILED=""
for row in $TARGETS; do
  target="${row%%|*}"
  builder="${row##*|}"

  if [ "$builder" = "cross" ] && ! docker info >/dev/null 2>&1; then
    echo "SKIP $target - Docker/colima not reachable"
    FAILED="$FAILED $target(no-docker)"
    continue
  fi

  echo "==> $target via $builder"
  # --locked matches CI (release.yml:158): without it a lockfile drift would
  # build different dependency versions than a real release.
  if $builder build --locked --release --target "$target" --all-features; then
    bin="target/$target/release/opencrabs"
    if [ -f "$bin" ]; then
      cp "$bin" "$OUT/opencrabs-${target}"
      echo "    ok -> $OUT/opencrabs-${target}"
    else
      echo "    BUILT but binary missing at $bin"
      FAILED="$FAILED $target(no-binary)"
    fi
  else
    echo "    FAILED $target"
    FAILED="$FAILED $target(build)"
  fi
  echo
done

echo "-- summary --"
ls -lh "$OUT" 2>/dev/null | tail -n +2 | awk '{printf "  %-8s %s\n", $5, $9}'
[ -n "$FAILED" ] && echo "  failed:$FAILED"
echo
echo "  not built: x86_64-pc-windows-msvc (needs Windows or CI)"
