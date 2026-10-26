# AGENTS.md

Guidance for coding agents (Claude Code, Codex, and others) working in this repository. `CLAUDE.md` is a symlink to this file.

## Overview

`skl` is a Rust CLI that keeps agent skills (directories holding a `SKILL.md`) in sync across machines. A manifest (`~/.config/skl/skills.toml`) lists the skills a machine should have and a lockfile (`skills.lock`) pins each git skill to a commit. Both are meant to live in dotfiles. Skills are unpacked into a content store under `~/.local/share/skl` and symlinked into each agent's skills directory (`~/.claude/skills` for Claude Code, `~/.agents/skills` for Codex, or any path).

Plugins (`[plugins.<name>]`) are pinned and unpacked into the store the same way, then installed through Claude Code's and Codex's own plugin CLIs from a marketplace skl generates per agent.

Commands: `sync`, `update`, `add`, `remove`, `list`, `profile`, `agents`. Skills can be limited to named profiles, and each machine keeps its own active profiles in `<data>/profile`. Commands that can write hold an exclusive `flock` on `<data>/run.lock`; `list`, `agents`, and `agents list` are read-only.

## Layout

- `src/main.rs`: clap CLI, `add`/`remove` (edit the manifest with `toml_edit` to keep comments), `list` (including skills skl doesn't manage), `profile`, the run lock, and `write_atomic`.
- `src/sync.rs`: the sync engine. Resolves refs, builds the new lock, unpacks skills into the store, reconciles links against `state.toml`, and garbage-collects unused store commits and repo caches.
- `src/app.rs`: bounded direct-argv app imports, temporary environment, and command snapshots.
- `src/export.rs`: app sources (a `command` export or a known `app` such as Xcode), their member snapshots, digests, and frontmatter normalization.
- `src/git.rs`: every git call (`ls-remote`, update diffs, bare cache repos, shallow fetch of one commit, `ls-tree`, `archive` piped to `tar`). Skill discovery and frontmatter `name` parsing.
- `src/manifest.rs`: parses and validates `skills.toml` (git `source`/`ref`/`subdir`, local `path`, per-skill `agents` and `profiles`).
- `src/lock.rs`: the versioned lockfile.
- `src/agents.rs`: known agents and their skills directories.
- `src/paths.rs`: every location, from `HOME`, `SKL_CONFIG_DIR`/`XDG_CONFIG_HOME` and `SKL_DATA_DIR`/`XDG_DATA_HOME`.
- `src/plugins.rs`: `[plugins]` entries, their lock entries, the marketplaces skl generates under `<data>/plugins/<agent>`, the Claude Code and Codex CLI adapters that install them, and the read-only plugin report `info`, `list`, and `doctor` share.
- `src/plugin.rs`: detects the plugin around skills that `add` and `discover` see, for the warning that points at `add --plugin`.
- `src/profile.rs`: resolves the active profiles from `--profile`, `SKL_PROFILE`, then the machine's saved setting.
- `tests/cli.rs`: end-to-end tests that run the built binary.
- `examples/skills.toml`: annotated starter manifest for users.
- `tools/`: release maintainer binaries, enabled by the `maintainer-tools` feature.

## Commands

```bash
cargo build --all-features
cargo test --all-features
cargo fmt --check
cargo clippy --all-features --all-targets -- -D warnings
```

CI runs all of these on macOS and Linux with stable Rust, and builds with the MSRV (`rust-version` in `Cargo.toml`, currently 1.88 for let-chains). Releases are built by `.github/workflows/release.yml` when a `v*` tag is pushed.

## Conventions

- Pure Rust, no shell scripts. Git work shells out to the `git` binary so private repos use the machine's SSH keys and credential helpers, and exports go through `tar`. Do not add a git library.
- Unix only: `flock` via `libc` and `std::os::unix` symlinks.
- Write files with `write_atomic`, which writes through a temp file and follows symlinks so a manifest or lock linked from dotfiles stays linked.
- Edit the manifest with `toml_edit` so user formatting and comments survive.
- Errors use `anyhow` with context that names the file or skill. Output paths go through `Paths::tilde`.
- Keep `--dry-run` side-effect free: no lock, state, store or link changes.

## Safety invariants

These are tested in `tests/cli.rs` and must hold after any change.

- Only remove or repoint links recorded in `state.toml` with the target they still have. Anything else at a link path, including a user link into the store, is a conflict that is skipped unless `--force` moves it into a fresh directory under `<data>/backup`. Backups never overwrite each other.
- Record links in `state.toml` before creating them, so an interrupted run still knows what it owns.
- Never publish a store directory without a regular-file `SKILL.md`. Unpack to a temp directory and rename into place.
- A skill that fails to resolve, fetch or unpack keeps its previous lock entry, store content and links. A missing ordinary local skill keeps its links. Missing command executables, unavailable apps (including a Mac without Xcode), and absent optional local directories remove only owned links; command lock entries and snapshots stay cached.
- Garbage collection keeps every store commit and repo cache referenced by the lock or by an owned link.
- `--locked` fails rather than changing the lock.
- Plugin installs are recorded in `<data>/plugins/state.toml` before the harness CLI runs, and only recorded installs are updated or uninstalled. The same plugin from another marketplace is a conflict, never touched. Dry runs run no harness CLI, and tests use the fake `claude`/`codex` in `tests/fixtures/harness.rs`, never real ones.
- Manifest commands, including app source exports, run only during explicit sync/update, never in dry runs, locked syncs, read-only commands, or automatic sync after edits. Saved plans run only built-in app exports such as Xcode. Command failure preserves the previous install; imports reject symlinks and special files and obey time/output limits.

## Tests

Tests build throwaway git repos and reach them as `file://` URLs, with a temp `HOME` and the `XDG_*`/`SKL_*` overrides removed, so they need no network and never touch the real home directory. Test commits pass `-c user.name`, `-c user.email` and `-c commit.gpgsign=false`, so no git identity is required. Add a test in `tests/cli.rs` for any behavior change, especially around link ownership.
