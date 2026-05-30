# Deployment

Hoststamp is local-first. Exposing it beyond localhost requires enabling API
auth and placing it behind normal production network controls.

## Baseline Hardening

For shared or remote deployments:

- Set `api.auth.required = true` or `HOSTSTAMP_API_AUTH_REQUIRED=true`.
- Provide `HOSTSTAMP_ADMIN_TOKEN` from a secret manager or environment.
- Provide `HOSTSTAMP_TOKEN_HASH_KEY` from a secret manager or environment.
- Put Hoststamp behind a reverse proxy that terminates TLS.
- Persist the SQLite database on a volume.
- Do not publish broad CORS headers unless a real browser client needs them.

Hoststamp does not implement in-process rate limiting. That is deliberate for
now: local counters would be misleading in a future multi-server deployment
without shared rate-limit state. Use reverse proxy, gateway, or platform-level
rate limiting if an exposed deployment needs it.

## Request Limits And Headers

The API caps JSON request bodies at 256 KiB. Current admin profile exports are
well below that limit; the cap exists to keep admin import and config endpoints
bounded.

The local UX response sets:

- `X-Content-Type-Options: nosniff`
- `X-Frame-Options: DENY`
- `Referrer-Policy: no-referrer`
- `Content-Security-Policy` with `frame-ancestors 'none'`
- `Permissions-Policy` disabling camera, microphone, geolocation, and payment

The CSP allows inline script and style because the local UX is currently a
single bundled HTML file.

## Docker

The image runs as UID/GID `10001`, sets
`XDG_CONFIG_HOME=/home/hoststamp/.config`, and defaults the database to
`/home/hoststamp/.config/hoststamp/hoststamp.db` when no config file is
mounted.

After `v0.1.0` is published, pull and run the stable image from GHCR:

```sh
docker pull ghcr.io/hoststamp/hoststamp:v0.1.0
docker run --rm -p 127.0.0.1:8080:8080 \
  --read-only \
  --tmpfs /tmp:rw,noexec,nosuid,size=16m \
  -v hoststamp-data:/home/hoststamp/.config/hoststamp \
  ghcr.io/hoststamp/hoststamp:v0.1.0
```

Build and smoke test locally with:

```sh
mise run docker-smoke
```

For local development without auth:

```sh
docker build -t hoststamp:dev .
docker run --rm -p 127.0.0.1:8080:8080 \
  --read-only \
  --tmpfs /tmp:rw,noexec,nosuid,size=16m \
  -v hoststamp-data:/home/hoststamp/.config/hoststamp \
  hoststamp:dev
```

For exposed containers, require auth and provide secrets through your runtime
secret manager or an uncommitted env file:

```sh
docker run --rm -p 8080:8080 \
  --read-only \
  --tmpfs /tmp:rw,noexec,nosuid,size=16m \
  --env-file ./hoststamp.env \
  -v hoststamp-data:/home/hoststamp/.config/hoststamp \
  ghcr.io/hoststamp/hoststamp:v0.1.0
```

Minimum env-file values for exposed use:

```sh
HOSTSTAMP_API_AUTH_REQUIRED=true
HOSTSTAMP_ADMIN_TOKEN=<secret>
HOSTSTAMP_TOKEN_HASH_KEY=<secret>
```

Do not commit the env file. Use a reverse proxy or platform load balancer for
TLS, request logging, and rate limiting.
