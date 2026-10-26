# Changelog

All notable changes to this project are documented here.

## [Unreleased]

### Added

- `subdir` on a declared Git source limits wildcard discovery, `skills` lists, and
  alias references to one directory of the repository, so a repository that ships
  the same skills in sibling folders (such as one copy per agent) can be tracked as
  `skills = "*"`. The lock records it with the wildcard's membership.
- `add` and `discover` accept a tree URL or `--subdir` naming a folder without a
  `SKILL.md` of its own and treat it as a collection of the skills beneath it.
  `add --all` on a collection declares one wildcard source scoped to the folder,
  named after its plugin or folder or `--alias NAME`, and excludes names the
  manifest already has. `discover` names the plugin the skills belong to and what
  else it ships.
- Plugins: `[plugins.<name>]` installs a whole Claude Code or Codex plugin, with
  its agents, commands, hooks, and MCP servers. skl pins it in the lock, unpacks it
  into the store, and installs it through each agent's own plugin CLI from a
  marketplace skl generates. Updates, `--locked`, dry runs, and removal work as
  they do for skills, and skl only updates or uninstalls the installs it recorded.
  `subdir` picks each agent's copy when a repository ships one per agent.
  `skl add owner/repo --plugin NAME` writes the entry. A plugin can't share its
  name with a skill. A lock with plugins is version 4.
- `add` warns when the skills it installs come from a plugin that also ships
  agents, commands, or hooks, and prints the `skl add --plugin` command that
  installs the whole plugin.
- Saved update plans cover plugins: `update --plan` pins each plugin's commit and
  includes the full diff of every folder an agent installs, without running an
  agent CLI. A plan with plugins is version 4, so an older skl refuses it.
- `info`, `list`, `doctor`, and `verify`, with or without `--json`, cover plugins:
  the pinned commit, each agent's folder, store copy, and install, plugins that
  aren't installed or are out of date, missing marketplace links, installs left
  after an agent is dropped, and store drift. None of them run an agent CLI.
- Cursor plugins: Cursor has no install command, so skl copies the plugin into
  `~/.cursor/plugins/local` when `~/.cursor` exists, and replaces or removes the
  copy only while it matches the digest skl recorded. An edited or foreign folder
  is a conflict that skl leaves in place. `info`, `list`, `doctor`, and `verify`
  report edited copies.

### Changed

- Name clashes between declarations name both sources and print the `exclude`
  line that keeps each copy, reporting every name two declarations share at once.
  Ambiguous names list each copy's directory and suggest the `subdir` or
  `--subdir` that picks one.

### Fixed

- Xcode and other app exports no longer intermittently fail with `No such file or
  directory (os error 2)` when temporary files disappear during a live export scan.
  Completed exports still undergo strict validation before installation. In 0.4.0
  this could happen on any sync with an Xcode source, whether or not the manifest
  had changed.

## [0.4.0] - 2026-10-20

### Added

- App sources: `[sources.<alias>]` with a `command` that exports several skills
  into `{output}`, selected with `skills`, `exclude`, and per-skill overrides like
  a Git source. Exports run only on explicit `sync` and `update`.

### Changed

- `[sources.<alias>]` replaces `[repositories.<alias>]`, which still works, and
  declares Git and app sources alike. Skills refer to a declared source with
  `source = "alias"` or `name = "alias"`, where a bare name without `/` or `:` is
  an alias and an unknown one is an error. `repository = "alias"` still works. Each
  alias is declared once across `[sources]`, `[repositories]`, and `[xcode]`.

- Xcode is an app source, `[sources.xcode]` with `app = "xcode"`, and `[xcode]`
  remains shorthand for it. Xcode skills gain `exclude`, per-skill overrides,
  `configure`, and `remove`. A Mac without Xcode, including one with only the
  Command Line Tools, skips its skills like any missing app instead of failing
  every sync. The export runs with the same limits and minimal environment as other
  app commands, keeping `DEVELOPER_DIR`.

- Lock version 3 writes each app source's command and app version once under
  `[sources.<alias>]`, with one content digest line per skill, so an Xcode skill
  takes one lock line, or two with `"*"`, instead of repeating the Xcode version.
  A `"*"` selection also records its complete export there, and Git wildcard
  coverage moves from `[repositories]` to `[sources]`. Version 1 and 2 locks
  migrate when next written, `sync --locked` leaves them untouched, and content
  cached under `store/xcode` is reused until the next export moves it.

- JSON output uses `schema_version: 2`. App source members report `origin: "app"`,
  their source's alias as `source`, and `command` provenance instead of `xcode`.
  `source-not-locked` replaces `xcode-not-locked` and `repository-not-locked`, and
  `verify` reports `source_kind: "app"`. Saved update plans use format version 3,
  with `source:ALIAS` destinations, and reject app sources with a `command`.

- `add` places skills from a declared Git source into that source, like `cargo add`
  inheriting a workspace dependency. Lists gain the name, alias-only sources gain a
  `name = "alias"` entry, and wildcard sources drop an exclusion. Refs,
  directories and agent, OS or profile settings the source doesn't already provide
  become a `source = "alias"` override. Sorted lists and `[skills]` tables stay
  sorted, and the output names any agents, OS or profile restrictions the skill
  inherits.

- `configure` and `remove` accept skills selected by a declared source's list, and
  `configure` also accepts cached wildcard members. Configuring a member writes a
  `source = "alias"` override entry and takes the name out of the list, so each
  name appears once. Resetting the last override returns it to the list or the
  wildcard.

### Fixed

- `skl --version` reports the release version. The 0.3.0 binaries reported 0.2.0,
  and a release now fails when its tag doesn't match the version in `Cargo.toml`.

## [0.3.0] - 2026-10-18

### Added

- App command imports with version provenance, content snapshots, bounded execution,
  cached locked syncs, and content diffs. Optional bundled paths skip absent apps.
  Version 2 locks read and migrate existing version 1 locks.

- Offline `verify [NAME ...] [--json]` for cached Git content and Xcode digests,
  reporting drift and unavailable baselines without modifying installations.

- Explicit `adopt NAME --from AGENT` snapshots unmanaged installed skills into
  portable local configuration, with dry-run and registration-only modes, source
  preservation, and ordinary sync conflict/backup handling.

- Exact-name exclusions for wildcard repository groups, with explicit overrides
  and persistent exclusions when removing discovered wildcard skills.

- `skl completions SHELL` generates shell completion scripts from the CLI definition
  on stdout without requiring configuration or changing installation state.

- Saved update plans now snapshot wildcard repository membership and self-contained
  Xcode content with full review diffs, verified hashes, and offline Xcode apply.
  Version 2 plans also validate generated destinations and protect their output paths.

- In-repository Homebrew formula for checksum-verified macOS and Linux release
  binaries on ARM64 and x86_64, with installation and update instructions.
- Automatic Homebrew formula updates after stable tag releases, with archive
  verification and protection against downgrading to older releases.

## [0.2.0] - 2026-10-15

### Added

- Repository aliases and persistent skill collections, including wildcard
  membership tracking and offline locked snapshots.
- Explicit portable project scope with installation state isolated from global
  skills and other projects.
- Support for more agent harnesses and configurable default agents.
- Local collection imports with `add DIRECTORY --all` or repeatable `--skill`,
  retaining live local paths and rejecting ambiguous selections before manifest edits.
- Opt-in `add --interactive` numbered skill and agent selection, with terminal-only
  prompts and cancellation before writes, plus read-only `agents detect` hints.

- GitHub/GitLab tree URLs and `owner/repo@skill` source shortcuts for `add` and
  `discover`, plus `add --subdir` for exact skill selection, including hidden
  directories and the repository root. Ambiguous slash refs require `--ref`.

- Read-only `skl search QUERY` queries the skills.sh catalog with optional owner
  filtering, result limits, safe installation suggestions, and versioned JSON.

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
