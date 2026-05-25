#!/bin/sh
# SPDX-License-Identifier: FSL-1.1-ALv2

set -eu

image="${1:-hoststamp:dev}"
container="${2:-hoststamp-smoke}"
volume="${3:-hoststamp-smoke-data}"
port="${4:-18080}"

cleanup() {
  docker rm -f "$container" >/dev/null 2>&1 || true
  docker volume rm "$volume" >/dev/null 2>&1 || true
}

trap cleanup EXIT INT TERM

docker volume create "$volume" >/dev/null
docker run --rm --detach \
  --name "$container" \
  --publish "127.0.0.1:${port}:8080" \
  --read-only \
  --tmpfs /tmp:rw,noexec,nosuid,size=16m \
  --mount "type=volume,src=${volume},dst=/home/hoststamp/.config/hoststamp" \
  "$image" >/dev/null

uid="$(docker exec "$container" /usr/bin/id -u)"
if [ "$uid" != "10001" ]; then
  echo "expected container UID 10001, got $uid" >&2
  exit 1
fi

i=0
while [ "$i" -lt 10 ]; do
  if curl --fail --silent --show-error "http://127.0.0.1:${port}/healthz"; then
    exit 0
  fi
  i=$((i + 1))
  sleep 1
done

docker logs "$container"
exit 1
