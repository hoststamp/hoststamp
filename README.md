Hoststamp
---

Hoststamp is a Rust CLI, API server, and local UX for generating deterministic
hostnames from profile-backed word pools and atomic counters.

## Quick Start

Run the local API and UX:

```sh
cargo run -p hoststamp -- serve
```

During development, run the watched server instead after `mise install --locked`:

```sh
mise run dev
```

The server binds to `127.0.0.1:8080` by default. Open:

- UX: `http://127.0.0.1:8080/`
- API health: `http://127.0.0.1:8080/api/health`
- OpenAPI JSON: `http://127.0.0.1:8080/api/openapi.json`
- OpenAPI YAML: `http://127.0.0.1:8080/api/openapi.yaml`

Generate names from the default profile:

```sh
cargo run -p hoststamp -- generate
cargo run -p hoststamp -- generate --count 10
cargo run -p hoststamp -- generate --count 10 --json
cargo run -p hoststamp -- --capacity --json
```

Generate stateless random names without opening the profile database:

```sh
cargo run -p hoststamp -- random
cargo run -p hoststamp -- random --count 10
cargo run -p hoststamp -- random --word1-lengths 4 --word2-lengths 4
```

Manage and use a named profile:

```sh
cargo run -p hoststamp -- --profile team-a profile new
cargo run -p hoststamp -- --profile team-a config set --word1-lengths 4,5,6 --word2-lengths 4,5,6
cargo run -p hoststamp -- --profile team-a profile clone team-a-test
cargo run -p hoststamp -- --profile team-a generate
cargo run -p hoststamp -- --profile team-a profile export > team-a.hoststamp-profile.json
cargo run -p hoststamp -- profile import team-a.hoststamp-profile.json
cargo run -p hoststamp -- backup export > hoststamp-backup.json
cargo run -p hoststamp -- --profile team-a lookup brief-cobra-db50d
cargo run -p hoststamp -- --profile team-a validate brief-cobra-db50d
cargo run -p hoststamp -- --profile team-a validate --file hostnames.txt --json
cargo run -p hoststamp -- --profile team-a fleet audit --file inventory.csv --json
cargo run -p hoststamp -- --profile team-a regenerate --atomic-value 42 --count 3 --json
```

Create the bootstrap config:

```sh
cargo run -p hoststamp -- config init
cargo run -p hoststamp -- config show
```

Print shell integration artifacts, including the top-level man page:

```sh
cargo run -p hoststamp -- openapi
cargo run -p hoststamp -- openapi --format yaml
cargo run -p hoststamp -- completions bash
cargo run -p hoststamp -- completions zsh
cargo run -p hoststamp -- completions fish
cargo run -p hoststamp -- man
```

After `v0.1.0` is published, the first stable Docker image will be available
from GHCR:

```sh
docker run --rm -p 127.0.0.1:8080:8080 \
  --read-only \
  --tmpfs /tmp:rw,noexec,nosuid,size=16m \
  -v hoststamp-data:/home/hoststamp/.config/hoststamp \
  ghcr.io/hoststamp/hoststamp:v0.1.0
```

## Core Model

Hostnames are assembled from `word1`, `word2`, and `suffix`. Profile-backed
generation stores configuration in SQLite, increments an atomic counter
transactionally, and derives each hostname from the profile UUID, profile
config hash, and atomic value.

Stored profile configs include `engine = "atomic-v1"`. That engine freezes the
deterministic generation contract: word-pair permutation, no-repeat word
handling, suffix encoding, profile-specific suffix alphabet derivation, and
`word1-word2-suffix` formatting. Future algorithm changes must use a new
engine value instead of changing `atomic-v1`.

The default profile seed is `5/5/5`: five-letter `word1`, five-letter `word2`,
and a minimum five-character lowercase base36 Sqids suffix. The default
profile slug is `_`.

## API And UX

Common local endpoints:

- `POST /api/generate?count=3`
- `GET /api/regenerate?atomic_value=42&count=3`
- `GET /api/lookup?hostname=brief-cobra-db50d`
- `GET /api/random?count=3`
- `GET /api/capacity?profile=_`
- `GET /api/profiles`
- `GET /api/openapi.json`
- `GET /api/openapi.yaml`

`POST /api/generate` returns newline-delimited `text/plain` by default so shell
clients can pipe the response directly. Pass `format=json` for structured
output. Profile-backed generation and regeneration also return:

- `x-hoststamp-profile`
- `x-hoststamp-atomic-values`

Admin endpoints require a configured admin bearer token. Profile-backed
generation can also require admin or profile bearer tokens when
`api.auth.required` or `HOSTSTAMP_API_AUTH_REQUIRED=true` is set. Profile
tokens can optionally expire with `expires_at_ms`.

## Documentation

- [Generation](docs/generation.md): deterministic naming, random generation,
  capacity math, and regeneration.
- [Configuration](docs/configuration.md): config file, environment variables,
  profiles, and storage.
- [API](docs/api.md): API routes, admin endpoints, auth behavior, and local UX.
- [Integrations](docs/integrations.md): GitHub Actions validation and fleet audit examples.
- [Dictionaries](docs/dictionaries.md): embedded categories, version hashes,
  and attribution.
- [Deployment](docs/deployment.md): exposed-server guidance, request limits,
  security headers, and Docker.
- [Release](docs/release.md): release prep PRs, stable publishing, nightly
  images, and release checks.
- [Development](docs/development.md): checks, crate layout, CI, and commit
  message conventions.

Release preparation is handled through an explicit local workflow instead of
GitHub Actions committing source changes. See [Release](docs/release.md) for
the version bump, pull request, stable release, and nightly image process.

## Project Commands

```sh
cargo check --all-targets
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test --all-targets
cargo llvm-cov --all-targets --locked --summary-only --fail-under-lines 90
cargo build --release --locked
docker build -t hoststamp:dev .
mise run docker-smoke
```

The same local checks are available through `mise`:

```sh
mise install --locked
mise run check
mise run ci
```

## License

Hoststamp source is licensed under the Functional Source License 1.1,
ALv2 Future License (`FSL-1.1-ALv2`). See [LICENSE](./LICENSE).

Third-party notices for bundled datasets are in
[THIRD-PARTY-NOTICES.md](./THIRD-PARTY-NOTICES.md) and are also available from
the CLI:

```sh
cargo run -p hoststamp -- --credits
```

Global inspection flags such as `--credits` and `--list-categories` are handled
before subcommands, similar to `--version`.

## Commit Messages

Use Conventional Commit-style subjects:

```text
<type>: <imperative summary>
```

Common prefixes:

- `docs`: documentation and repo guidance
- `feat`: user-facing features
- `fix`: bug fixes
- `ci`: CI and release automation
- `build`: build system, packaging, and dependency tooling
- `deps`: dependency updates
- `docker`: Docker image and base image updates
- `test`: tests and test infrastructure
- `refactor`: behavior-preserving code changes
