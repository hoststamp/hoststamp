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
cargo run -p hoststamp -- --list-categories
cargo run -p hoststamp -- generate
```

Generate hostnames:

```sh
cargo run -p hoststamp -- generate
cargo run -p hoststamp -- generate --count 10
cargo run -p hoststamp -- generate --word1-categories adjective --word2-categories animal
cargo run -p hoststamp -- generate --word1-categories adjective,noun --word2-categories animal,name
cargo run -p hoststamp -- generate --word1-lengths 4,5,6 --word2-lengths 4,5,6
cargo run -p hoststamp -- generate --word1-lengths any --word2-lengths any
cargo run -p hoststamp -- generate --suffix-length 10
cargo run -p hoststamp -- generate --no-suffix
cargo run -p hoststamp -- generate --no-word2 --no-suffix
```

Hostnames are assembled from three positions: `word1`, `word2`, and `suffix`.
The default shape is `word1-word2-suffix` (e.g. `5/5/5`) with `adjective` for
`word1` and `animal` for `word2`. Each word position has independent
disable, lengths, and categories controls (`--no-word1`, `--word1-lengths`,
`--word1-categories`, and the matching `word2` flags). The suffix has
`--no-suffix`, `--suffix-length`, `--suffix-source`, and `--suffix-hash`.
Words never repeat within a single hostname. `--count` is capped at 50.

`--wordN-categories` accepts a comma-separated category list. `--wordN-lengths`
accepts a comma-separated list of exact lengths or the literal `any` for no
length filter. Selection across selected categories and length buckets is
weighted by available word count so every candidate word has an even chance.
If the selected categories do not contain enough matching words, generation
fails loudly.

The suffix is a hex truncation of a hash. `--suffix-length` is bounded to
`[4, 40]` (SHA-1 hex length). `--suffix-source random` (default) hashes a
random UUID; `--suffix-source atomic` is reserved for the future deterministic
mode and is rejected today. `--suffix-hash sha1` is the only supported hash
for now.

Category stats from the generated artifact:

| Category | Available entries | Word lengths |
| --- | ---: | --- |
| `adjective` | 584 | 3-12 |
| `adverb` | 257 | 4-10 |
| `animal` | 448 | 3-8 |
| `deity` | 151 | 3-11 |
| `diceware` | 8,026 | 3-10 |
| `element` | 117 | 3-12 |
| `gemstone` | 312 | 3-12 |
| `metal` | 91 | 3-12 |
| `monster` | 20 | 5-11 |
| `name` | 652 | 3-12 |
| `noun` | 95 | 3-10 |
| `ocean` | 5 | 6-8 |
| `phonetic` | 26 | 4-8 |
| `planet` | 13 | 4-8 |
| `river` | 187 | 3-12 |
| `scientist` | 241 | 4-12 |
| `star` | 435 | 3-12 |
| `stone` | 48 | 4-12 |
| `tolkien` | 398 | 3-11 |
| `wind` | 90 | 3-12 |

Run `hoststamp --list-categories` for the category names and total counts
compiled into the binary.

Local endpoints:

- UX: `http://127.0.0.1:8080/`
- API health: `http://127.0.0.1:8080/api/health`
- API generate: `http://127.0.0.1:8080/api/generate?count=3`
- Container health: `http://127.0.0.1:8080/healthz`

`/api/generate` returns JSON with a `hostnames` array. Query parameters mirror
the generator config names, including `count`, `word1_lengths`,
`word1_categories`, `word2_lengths`, `word2_categories`, `suffix_enabled`,
`suffix_length`, `suffix_source`, and `suffix_hash`.

### Configuration

Hoststamp reads configuration from the first available source:

1. `--config <path>`
2. `HOSTSTAMP_CONFIG=<path>`
3. `$XDG_CONFIG_HOME/hoststamp/config.toml`
4. `~/.config/hoststamp/config.toml`
5. Built-in defaults

CLI flags override environment variables, environment variables override
the config file, and the config file overrides built-in defaults. Every
generator field is settable through all three layers.

```toml
[server]
addr = "127.0.0.1:8080"

[generate]
count = 1

[generate.word1]
enabled    = true
lengths    = [5]                  # omit (or set "any") to allow any length
categories = ["adjective"]

[generate.word2]
enabled    = true
lengths    = [5]
categories = ["animal"]

[generate.suffix]
enabled = true
length  = 5                       # bounded to [4, 40]
source  = "random"                # "atomic" reserved for the future profile layer
hash    = "sha1"
```

Environment variables (all `HOSTSTAMP_*`):

| Env var | Maps to |
| --- | --- |
| `HOSTSTAMP_CONFIG` | path to the config file |
| `HOSTSTAMP_ADDR` | `server.addr` |
| `HOSTSTAMP_COUNT` | `generate.count` |
| `HOSTSTAMP_WORD1_ENABLED` | `generate.word1.enabled` (`true`/`false`) |
| `HOSTSTAMP_WORD1_LENGTHS` | `generate.word1.lengths` (csv ints or `any`) |
| `HOSTSTAMP_WORD1_CATEGORIES` | `generate.word1.categories` (csv) |
| `HOSTSTAMP_WORD2_ENABLED` / `_LENGTHS` / `_CATEGORIES` | mirror of word1 |
| `HOSTSTAMP_SUFFIX_ENABLED` | `generate.suffix.enabled` |
| `HOSTSTAMP_SUFFIX_LENGTH` | `generate.suffix.length` |
| `HOSTSTAMP_SUFFIX_SOURCE` | `generate.suffix.source` |
| `HOSTSTAMP_SUFFIX_HASH` | `generate.suffix.hash` |

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
third-party notice drift, workflow syntax, and the Docker image. Pull requests
run a fast amd64 Docker smoke build. Pushes to `main` publish multi-arch
nightly images to GHCR tagged as `nightly`, `sha-<short>`, and
`vX.Y.Z-nightly.YYYYMMDD.N`. Cargo audit and Dependabot run weekly.

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
