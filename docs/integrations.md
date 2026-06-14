# Integrations

Hoststamp can validate profile-backed names in CI without running the local UX
or exposing the API. The validation job needs two non-secret inputs:

- A portable profile export created with `hoststamp --profile <slug> profile export`
- A newline-delimited file containing candidate or inventory hostnames

The profile export includes the profile UUID, config, config hash, access mode,
and last issued atomic value. It does not include profile token secrets. Treat
profile export changes like infrastructure policy changes: require normal review
or CODEOWNERS approval before merging them.

## GitHub Actions

The example workflow in
[`docs/examples/github-actions/validate-hostnames.yml`](examples/github-actions/validate-hostnames.yml)
imports the canonical profile export from `origin/main`, validates a hostname
inventory file from the pull request, and fails the pull request when any
hostname is invalid.

Example repository layout:

```text
.github/
  hoststamp/
    team-a.profile.json
  workflows/
    validate-hostnames.yml
infrastructure/
  hostnames.txt
```

Create the profile export from the source-of-truth profile:

```sh
hoststamp --profile team-a profile export > .github/hoststamp/team-a.profile.json
```

The hostname file is one hostname per line. Blank lines are ignored:

```text
brief-cobra-db50d
local-panda-l9t23
```

Copy the example workflow into `.github/workflows/validate-hostnames.yml`, then
adjust the profile slug, profile export path, and hostname file path for the
repository using it. The example treats `origin/main` as the trusted profile
source so a pull request cannot weaken validation by changing the profile export
and hostname list together. If a repository imports the profile export from the
pull request checkout instead, protect `.github/hoststamp/**` with CODEOWNERS
and required-review branch protection; without that protection, the check is
only advisory.

The example pins the checkout action to a full commit SHA, pins the Rust
toolchain version, and installs Hoststamp from the `v0.2.0` tag. Update those
refs deliberately when upgrading. Replace the install step with the team's
pinned release archive, package, or container installation when one is
standardized.

The validation step uses a throwaway SQLite database under `${{ runner.temp }}`.
It imports the trusted profile export on each run, then calls:

```sh
hoststamp --profile team-a validate --file infrastructure/hostnames.txt --json
```

`hoststamp validate` exits non-zero if any hostname is invalid, so no extra JSON
parsing is required to fail the workflow. Keep the `--json` flag when downstream
workflow steps need machine-readable validation details.

For larger inventories, use `hoststamp fleet audit --file <path> --json`
instead. Fleet audit accepts newline-delimited text, CSV with a `hostname`
column, JSON arrays of hostnames or objects, or a JSON object with a
`hostnames` array. It reports the same validity and atomic-value details, adds
duplicate detection, and exits non-zero when the inventory contains invalid or
repeated hostnames.
