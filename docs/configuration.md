# Configuration

Hoststamp reads bootstrap configuration from the first available source:

1. `--config <path>`
2. `HOSTSTAMP_CONFIG=<path>`
3. `$XDG_CONFIG_HOME/hoststamp/config.toml`
4. `~/.config/hoststamp/config.toml`
5. Built-in defaults

The bootstrap config handles server and storage settings. Generator profile
defaults are stored in the profile database. The default database path is
`$XDG_CONFIG_HOME/hoststamp/hoststamp.db`, falling back to
`~/.config/hoststamp/hoststamp.db`; it sits next to the default config file.

## Bootstrap Config

Create the bootstrap config with:

```sh
hoststamp config init
hoststamp --config /etc/hoststamp/config.toml config init
```

`config init` creates parent directories as needed, refuses to overwrite an
existing file, and creates the config as owner-readable only on Unix. Use
OpenSSL to create 32-character secret values:

```sh
openssl rand -base64 24
```

```toml
[server]
# addr = "127.0.0.1:8080"

[storage]
# Defaults to hoststamp.db next to this config file.
# url = "sqlite:///home/hoststamp/.config/hoststamp/hoststamp.db"

[api.auth]
# Disabled by default for local development.
required = false

# For local single-user setups, uncomment and set direct secret values here.
# For shared systems, keep secrets in environment variables or a secret manager.
# If secrets are stored here, keep this file private with chmod 600.
# admin_token = "replace-with-openssl-output"
# token_hash_key = "replace-with-openssl-output"

# Environment variables override direct secret values when both are present.
admin_token_env = "HOSTSTAMP_ADMIN_TOKEN"
token_hash_key_env = "HOSTSTAMP_TOKEN_HASH_KEY"

# Example:
#   export HOSTSTAMP_ADMIN_TOKEN="$(openssl rand -base64 24)"
#   export HOSTSTAMP_TOKEN_HASH_KEY="$(openssl rand -base64 24)"
```

Environment variables all use the `HOSTSTAMP_*` prefix:

| Env var | Maps to |
| --- | --- |
| `HOSTSTAMP_CONFIG` | path to the config file |
| `HOSTSTAMP_ADDR` | `server.addr` |
| `HOSTSTAMP_DATABASE_URL` | `storage.url` |
| `HOSTSTAMP_API_AUTH_REQUIRED` | `api.auth.required` |
| `HOSTSTAMP_ADMIN_TOKEN` | admin bearer token secret |
| `HOSTSTAMP_TOKEN_HASH_KEY` | HMAC key for profile token hashes |

Use `hoststamp config show` to print the resolved bootstrap settings, selected
profile metadata, stored profile config, and effective generator config after
request options such as `--count` are applied. Database URLs that could contain
secrets are redacted, and auth secrets are shown only as configured/not
configured booleans.

## Profiles

Profiles are selected with `--profile <slug>`. The default profile slug is `_`,
which is reserved and cannot be used as a normal user slug. User slugs use
lowercase ASCII letters, digits, and hyphens, and must start and end with a
letter or digit. Missing profiles are seeded from the built-in `5/5/5`
generator defaults on first use.

Profile management commands operate on active profile rows:

```sh
hoststamp profile list
hoststamp --profile team-a profile show
hoststamp --profile team-a profile new
hoststamp --profile team-a profile delete
hoststamp --profile team-a profile export > team-a.hoststamp-profile.json
hoststamp profile import team-a.hoststamp-profile.json
hoststamp --profile team-a profile set-access --access public
hoststamp --profile team-a profile token create --name deploy
hoststamp --profile team-a profile token create --name deploy --expires-at-ms 1893456000000
hoststamp --profile team-a profile token list
hoststamp --profile team-a profile token revoke <token-id>
hoststamp --profile team-a profile reset-atomic-value --atomic-value 999
```

`profile export` writes portable JSON containing the profile UUID, slug, access
mode, last issued atomic value, config hash, and config. `profile import` reads
that export and restores the same deterministic profile identity on another
machine. `profile delete`, `profile import` when replacing an existing differing
profile, and `profile reset-atomic-value` require two interactive confirmations.
`reset-atomic-value` sets the stored `last_atomic_value`; the next
profile-backed generation increments first and uses the following value. For
example, resetting to `999` makes the next generated hostname use atomic value
`1000`. Lowering the stored value can duplicate previously issued names, and
raising it skips part of the deterministic sequence.

Profile token names must be 64 characters or fewer, use lowercase ASCII
letters, digits, hyphen, underscore, or dot, and start and end with a letter or
digit. Active token names cannot be duplicated within one profile.

Profile-backed suffix generation treats the selected profile config as part of
the identity used for deterministic suffixes. Persistent generator settings are
changed with `hoststamp config set`, which asks for two confirmations before
replacing the active profile row. Replacement creates a new profile UUID and
resets that profile's atomic counter. `--count` is a request option only and
does not trigger profile replacement. API generation requests cannot override
stored profile config; use the admin config endpoint or CLI to replace profile
config deliberately.

## Shell Integration

Hoststamp can print shell completions and its generated top-level man page to
stdout:

```sh
hoststamp completions bash > hoststamp.bash
hoststamp completions zsh > _hoststamp
hoststamp completions fish > hoststamp.fish
hoststamp man > hoststamp.1
```

Install the generated files using the conventions of your shell or operating
system package. `hoststamp man` emits `hoststamp(1)` only; it does not emit
separate per-subcommand pages. The completion command supports `bash`, `zsh`,
and `fish`.

## Storage

SQLite storage is implemented for local profiles. `postgres://` and
`postgresql://` URLs are recognized as planned remote storage backends, but
Postgres execution is not implemented yet.
