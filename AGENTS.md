## 1. Think Before Coding
Don't assume. Don't hide confusion. Surface tradeoffs.

Before implementing:
- State your assumptions explicitly. If uncertain, ask.
- If multiple interpretations exist, present them. Do not pick silently.
- If a simpler approach exists, say so. Push back when warranted.
- If something is unclear, stop. Name what is confusing. Ask.

## 2. Simplicity First
Minimum code that solves the problem. Nothing speculative.

- No features beyond what was asked.
- No abstractions for single-use code.
- No flexibility or configurability that was not requested.
- No error handling for impossible scenarios.
- If you write 200 lines and it could be 50, rewrite it.

Ask yourself: Would a senior engineer say this is overcomplicated? If yes, simplify.

## 3. Surgical Changes
Touch only what you must. Clean up only your own mess.

When editing existing code:
- Do not improve adjacent code, comments, or formatting.
- Do not refactor things that are not broken.
- Match existing style, even if you would do it differently.
- If you notice unrelated dead code, mention it. Do not delete it.

When your changes create orphans:
- Remove imports, variables, or functions that YOUR changes made unused.
- Do not remove pre-existing dead code unless asked.

The test: Every changed line should trace directly to the user's request.

## 4. Goal-Driven Execution
Define success criteria. Loop until verified.

Transform tasks into verifiable goals:
- "Add validation" becomes "Write tests for invalid inputs, then make them pass"
- "Fix the bug" becomes "Write a test that reproduces it, then make it pass"
- "Refactor X" becomes "Ensure tests pass before and after"

For multi-step tasks, state a brief plan:
1. [Step] -> verify: [check]
2. [Step] -> verify: [check]
3. [Step] -> verify: [check]

Strong success criteria let you loop independently. Weak criteria require constant clarification.

## 5. Destructive Actions Need Explicit Instruction
Don't mutate state outside the working tree without being told to.

- Includes `git commit`, `git push`, `git rebase`, `git reset --hard`, `git clean`, branch or file deletion, and dependency installs that modify lockfiles.
- Drafting an artifact is not an instruction to apply it. Writing a commit message is not an instruction to commit. Writing a script is not an instruction to run it.
- Default action after producing an artifact is to present it. The user applies it.

## Project Commands
- Build: `cargo build --release --locked`
- Test: `cargo test --all-targets`
- Lint: `cargo clippy --all-targets -- -D warnings`
- Format check: `cargo fmt --all -- --check`
- Coverage: `cargo llvm-cov --all-targets --locked --summary-only --fail-under-lines 60`
- Docker build: `docker build -t hoststamp:dev .`
- Dev server: `cargo run -p hoststamp -- serve`

If the local shell refuses to load `.mise.toml` because it is not trusted, prefix commands with `MISE_TRUSTED_CONFIG_PATHS=$PWD` instead of changing global trust state.

## Project Conventions
- Use `Hoststamp` for product and documentation prose.
- Use `hoststamp` for machine-readable identifiers such as package names, binary names, repositories, domains, Docker images, and config keys.
- Avoid `HostStamp` unless a specific external integration or legacy artifact requires that spelling.
- Hoststamp source files use `SPDX-License-Identifier: FSL-1.1-ALv2` unless legal review changes the plan.
- Config precedence is CLI flags, then environment variables, then config file, then built-in defaults.
- The default user config path is `$XDG_CONFIG_HOME/hoststamp/config.toml`, falling back to `~/.config/hoststamp/config.toml`.

## Workspace Hygiene
- Do not commit scratch notes, transcripts, one-off plans, or temporary agent artifacts.
- Do not reference private local workspace paths in committed source code, comments, docstrings, or documentation.
