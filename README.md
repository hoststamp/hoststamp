Hoststamp
---

Hoststamp is a Rust CLI, API server, and local UX.

## Development

### Run locally

```sh
cargo run -p hoststamp -- serve
```

The server binds to `127.0.0.1:8080` by default. Set `HOSTSTAMP_ADDR`
or pass `--addr` to choose another bind address.

```sh
cargo run -p hoststamp -- serve --addr 0.0.0.0:8080
cargo run -p hoststamp -- health
cargo run -p hoststamp -- --version
cargo run -p hoststamp -- --credits
```

Local endpoints:

- UX: `http://127.0.0.1:8080/`
- API health: `http://127.0.0.1:8080/api/health`
- Container health: `http://127.0.0.1:8080/healthz`

### Configuration

Hoststamp reads configuration from the first available source:

1. `--config <path>`
2. `HOSTSTAMP_CONFIG=<path>`
3. `$XDG_CONFIG_HOME/hoststamp/config.toml`
4. `~/.config/hoststamp/config.toml`
5. Built-in defaults

CLI flags override environment variables, environment variables override
the config file, and the config file overrides built-in defaults.

```toml
[server]
addr = "127.0.0.1:8080"
```

For containers, mount a config file and set `HOSTSTAMP_CONFIG`:

```sh
docker run --rm -p 8080:8080 \
  -e HOSTSTAMP_CONFIG=/etc/hoststamp/config.toml \
  -v "$PWD/config.example.toml:/etc/hoststamp/config.toml:ro" \
  hoststamp:dev
```

### Project commands

```sh
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test --all-targets
cargo build --release --locked
docker build -t hoststamp:dev .
```

### Docker

```sh
docker build -t hoststamp:dev .
docker run --rm -p 8080:8080 hoststamp:dev
```

### Automation

CI validates formatting, clippy, tests with coverage, release builds,
workflow syntax, and the Docker image. Pushes to `main` publish multi-arch
nightly images to GHCR tagged as `nightly` and `sha-<short>`. Cargo audit
and Dependabot run weekly.

## License

Hoststamp source is licensed under the Functional Source License 1.1,
ALv2 Future License (`FSL-1.1-ALv2`). See [LICENSE](./LICENSE).

Third-party notices for bundled datasets are in
[THIRD-PARTY-NOTICES.md](./THIRD-PARTY-NOTICES.md) and are also available
from the CLI:

```sh
cargo run -p hoststamp -- --credits
```

### Commit messages

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
