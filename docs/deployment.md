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

For containers, mount a config file and set `HOSTSTAMP_CONFIG`:

```sh
docker run --rm -p 8080:8080 \
  -e HOSTSTAMP_CONFIG=/etc/hoststamp/config.toml \
  -e HOSTSTAMP_DATABASE_URL=sqlite:///home/hoststamp/.config/hoststamp/hoststamp.db \
  -v hoststamp-data:/home/hoststamp/.config/hoststamp \
  -v "$PWD/config.example.toml:/etc/hoststamp/config.toml:ro" \
  hoststamp:dev
```

Build locally with:

```sh
docker build -t hoststamp:dev .
docker run --rm -p 8080:8080 hoststamp:dev
```
