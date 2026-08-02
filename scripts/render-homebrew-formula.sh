#!/usr/bin/env bash
# Render the Homebrew formula from its template and a SHA256SUMS file (#924).
#
# Kept as a script rather than inline YAML so it can be run locally against a
# published release to see exactly what the tap will receive, and so a failure
# names the missing piece instead of producing a formula with an empty hash.
#
# Usage: render-homebrew-formula.sh <version> <sha256sums-file> [output]
#   version          without the leading v, e.g. 0.3.78
#   sha256sums-file  the SHA256SUMS published with the release
set -euo pipefail

VERSION="${1:?version required (without leading v)}"
SUMS="${2:?path to SHA256SUMS required}"
OUT="${3:-opencrabs.rb}"

TEMPLATE="$(dirname "$0")/../packaging/homebrew/opencrabs.rb.template"
[ -f "$TEMPLATE" ] || { echo "template not found: $TEMPLATE" >&2; exit 1; }
[ -f "$SUMS" ] || { echo "checksums not found: $SUMS" >&2; exit 1; }

# Pull one hash by asset suffix. A missing entry is fatal: Homebrew rejects a
# mismatched sha256 outright, so shipping a formula with a blank or wrong hash
# breaks every install of that platform rather than degrading.
sha_for() {
  local suffix="$1" hash
  hash="$(awk -v s="opencrabs-v${VERSION}-${suffix}.tar.gz" '$2 == s || $2 == "*"s {print $1}' "$SUMS" | head -1)"
  if [ -z "$hash" ]; then
    echo "no checksum for opencrabs-v${VERSION}-${suffix}.tar.gz in $SUMS" >&2
    exit 1
  fi
  printf '%s' "$hash"
}

MACOS_ARM64="$(sha_for macos-arm64)"
MACOS_AMD64="$(sha_for macos-amd64)"
LINUX_ARM64="$(sha_for linux-arm64)"
LINUX_AMD64="$(sha_for linux-amd64)"

sed \
  -e "s/@VERSION@/${VERSION}/g" \
  -e "s/@SHA_MACOS_ARM64@/${MACOS_ARM64}/g" \
  -e "s/@SHA_MACOS_AMD64@/${MACOS_AMD64}/g" \
  -e "s/@SHA_LINUX_ARM64@/${LINUX_ARM64}/g" \
  -e "s/@SHA_LINUX_AMD64@/${LINUX_AMD64}/g" \
  "$TEMPLATE" > "$OUT"

# An unreplaced placeholder means the template gained a field the renderer does
# not know about. Better to fail here than to publish a formula containing a
# literal @SHA_...@ that fails at install time on a user's machine.
if grep -q '@[A-Z_]*@' "$OUT"; then
  echo "unreplaced placeholder in $OUT:" >&2
  grep -n '@[A-Z_]*@' "$OUT" >&2
  exit 1
fi

echo "rendered $OUT for v${VERSION}"
