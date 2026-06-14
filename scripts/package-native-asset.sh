#!/bin/sh
# SPDX-License-Identifier: FSL-1.1-ALv2

set -eu

usage() {
  cat <<'USAGE'
Usage: sh scripts/package-native-asset.sh vX.Y.Z <asset> <output-dir>

Packages the release binary and documentation into:
  <output-dir>/hoststamp-vX.Y.Z-<asset>.tar.gz
USAGE
}

die() {
  echo "package-native-asset: $*" >&2
  exit 1
}

if [ "${1:-}" = "-h" ] || [ "${1:-}" = "--help" ]; then
  usage
  exit 0
fi

if [ "$#" -ne 3 ]; then
  usage >&2
  exit 2
fi

tag="$1"
asset="$2"
out_dir="$3"
binary="${HOSTSTAMP_BINARY:-target/release/hoststamp}"

printf '%s\n' "$tag" | grep -Eq '^v(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$' \
  || die "tag must be stable vX.Y.Z, got $tag"

case "$asset" in
  ""|*[!abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789._-]*)
    die "asset must contain only letters, numbers, dots, underscores, and hyphens"
    ;;
esac

[ -n "$out_dir" ] || die "output directory must not be empty"
[ -f "$binary" ] || die "binary not found: $binary"
[ -x "$binary" ] || die "binary is not executable: $binary"

for file in README.md LICENSE THIRD-PARTY-NOTICES.md; do
  [ -f "$file" ] || die "required file not found: $file"
done

name="hoststamp-${tag}-${asset}"
package_dir="$out_dir/$name"
archive="$out_dir/$name.tar.gz"

mkdir -p "$out_dir"
[ ! -e "$package_dir" ] || die "package directory already exists: $package_dir"
[ ! -e "$archive" ] || die "archive already exists: $archive"

mkdir "$package_dir"
cp "$binary" "$package_dir/hoststamp"
cp README.md LICENSE THIRD-PARTY-NOTICES.md "$package_dir/"
tar -C "$out_dir" -czf "$archive" "$name"
