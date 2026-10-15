# Changelog

All notable changes to this project are documented here.

## [Unreleased]

### Added

- Local collection imports with `add DIRECTORY --all` or repeatable `--skill`,
  retaining live local paths and rejecting ambiguous selections before manifest edits.
- GitHub/GitLab tree URLs and `owner/repo@skill` source shortcuts for `add` and
  `discover`, plus `add --subdir` for exact skill selection, including hidden
  directories and the repository root. Ambiguous slash refs require `--ref`.


- Saved Git update plans with `update --plan FILE` and `update --apply-plan FILE`: review full patches, install exact commits, and reject stale manifest, lock or machine context. Planning writes only a new plan file and does not support Xcode manifests.
- Versioned JSON output for `skl list`, `skl info`, and `skl doctor`, including
  selection, provenance, ownership, unmanaged entries, and structured errors.

- Read-only `skl info <name>` shows source and pin provenance, effective profile/OS
  selection, content availability, and actual link ownership without fetching or
  exporting skills. It also inspects unmanaged entries in configured agent directories.
- Read-only `skl doctor` diagnostics for local installation health, link ownership,
  cached skills, configuration errors, and missing tools, with actionable remedies.
- `skl discover SOURCE [--ref REF]` browses repository skills, descriptions and
  subdirectories at a resolved commit without changing installed state.
- Per-skill `--agent`, `--os`, and `--only-profile` flags for `add`, plus
  `configure <names...>` to update existing entries with explicit reset flags
  and optional `--no-sync`. Batch edits preserve comments and symlinked manifests
  and validate the complete manifest before writing.

- Xcode skill imports through `xcrun agent skills export`: configure `[xcode]`
  with `skills = "*"` or selected names, plus optional agents and profiles.
  Sync records Xcode version/build and content hashes, preserves previous installs
  when exports fail or selected skills disappear, and normalizes frontmatter field
  order to avoid spurious updates. Dry runs skip exporting; `--locked` reuses cached
  snapshots without invoking Xcode and requires verified wildcard coverage.
- Operating-system filters via `os = ["macos"]` or `os = ["linux"]` for skills
  and Xcode imports, combined with profiles. Xcode imports default to macOS;
  excluded skills keep their lock entries and have managed links removed.
- Linux ARM64 (glibc) release binaries and CI coverage.

### Fixed

- Skill discovery retains unique hidden skills, including `.curated` and
  `.experimental` collections, while suppressing hidden mirrors of visible names.
  Ambiguous Git additions fail before editing the manifest.

- Automatically version unreleased changelog entries when a release tag is pushed
  and save the updated changelog to the default branch.

- Publish the matching changelog entry as the notes for each GitHub release.
- Ignore relative `XDG_CONFIG_HOME` and `XDG_DATA_HOME` values and fall back to
  the default directories, as required by the XDG Base Directory Specification.

## [0.1.0] - 2026-10-14

Initial release.

### Added

- `skl sync` installs, links and prunes skills to match `~/.config/skl/skills.toml`
  and `skills.lock`, linking each skill into Claude Code (`~/.claude/skills`), Codex
  (`~/.agents/skills`) or any directory path.
- `skl add` adds skills from `owner/repo`, a git URL or a local directory, picking
  skills by frontmatter name with `--skill`, `--all` and `--ref`, then syncs unless
  `--no-sync` is passed.
- `skl remove` drops skills from the manifest, unlinks them and cleans up the store.
- `skl update` moves some or all skills to the newest commit of their branch, tag or
  default branch and prints which files changed in each. `--patch` prints the full diff.
- `skl list` shows each skill's locked commit and link status, then anything in the
  agents' skill directories that skl doesn't manage.
- Lockfile pinning: every git skill is pinned to an exact commit, so each machine
  installs the same content until `skl update` runs.
- Link ownership: skl records the links it creates in `state.toml` and only repoints
  or removes those. Anything else in the way is reported as a conflict.
- `--force` moves conflicting files and links into a fresh backup directory under
  `~/.local/share/skl/backup` instead of skipping them.
- `--locked` fails instead of changing the lock, and `--dry-run` shows what would
  change without touching the lock, store or links.
- Local path skills link straight to their directory, with relative paths resolved
  against the manifest's directory.
- Per-skill `agents` lists, plus a manifest-wide default.
- Profiles: skills can list `profiles`, and each machine picks its own with
  `skl profile set`, `SKL_PROFILE` or `--profile`. Skills outside the active profiles
  keep their lock entries untouched.
- A run lock, so concurrent skl runs take turns instead of racing.
- Failed skills keep their previous lock entry, store content and links, so a flaky
  network or a broken upstream never uninstalls a working skill.
- Atomic writes that keep a symlinked manifest or lock linked.
