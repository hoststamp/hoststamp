#!/bin/sh
# SPDX-License-Identifier: FSL-1.1-ALv2

set -eu

die() {
  echo "dev-env: $*" >&2
  exit 1
}

generate_secret() {
  if ! command -v openssl >/dev/null 2>&1; then
    die "openssl is required to generate local development secrets"
  fi
  openssl rand -base64 24
}

ensure_secret_file() {
  path="$1"
  if [ ! -s "$path" ]; then
    old_umask="$(umask)"
    umask 077
    generate_secret >"$path"
    umask "$old_umask"
  fi
  chmod 600 "$path"
}

if [ "$#" -eq 0 ]; then
  die "expected a command to run"
fi

dev_dir="${HOSTSTAMP_DEV_DIR:-target/dev}"
mkdir -p "$dev_dir"

admin_token_file="$dev_dir/admin-token"
token_hash_key_file="$dev_dir/token-hash-key"

if [ -z "${HOSTSTAMP_ADMIN_TOKEN+x}" ]; then
  ensure_secret_file "$admin_token_file"
  HOSTSTAMP_ADMIN_TOKEN="$(cat "$admin_token_file")"
  export HOSTSTAMP_ADMIN_TOKEN
fi

if [ -z "${HOSTSTAMP_TOKEN_HASH_KEY+x}" ]; then
  ensure_secret_file "$token_hash_key_file"
  HOSTSTAMP_TOKEN_HASH_KEY="$(cat "$token_hash_key_file")"
  export HOSTSTAMP_TOKEN_HASH_KEY
fi

export HOSTSTAMP_DATABASE_URL="${HOSTSTAMP_DATABASE_URL:-$dev_dir/hoststamp.db}"
export HOSTSTAMP_UX_STATIC_DIR="${HOSTSTAMP_UX_STATIC_DIR:-crates/hoststamp-ux/static}"

echo "dev-env: using database $HOSTSTAMP_DATABASE_URL" >&2
echo "dev-env: admin token file $admin_token_file" >&2

exec "$@"
