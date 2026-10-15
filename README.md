# skl

`skl` keeps the agent skills on your Macs in sync. A manifest lists the skills a
machine should have, a lockfile pins each one to a commit, and `skl sync` makes the
machine match both: it fetches what's missing, links each skill into every agent
(Claude Code and Codex), and removes what you dropped.

Both files live in `~/.config/skl`, so they go in your dotfiles. On a new machine,
pulling dotfiles and running `skl sync` installs the same skills at the same commits.

## Why

Installing a skill is easy. Keeping several machines on the same skills is not:
each needs the same versions, skills you drop should disappear everywhere, and
nothing you put in an agent's directory by hand should be overwritten. `skl` pins
every skill to a commit, prunes what's no longer wanted, and never modifies or
deletes anything it didn't create.

## Install

```sh
cargo install --git https://github.com/nanoxd/skl
```

Prebuilt binaries for macOS (Apple Silicon and Intel) and Linux (x86_64 and ARM64, glibc) are attached to each
[release](https://github.com/nanoxd/skl/releases). `skl` needs `git` and `tar` on
your `PATH`.

In GitHub Actions, install an exact release without a Rust toolchain:

```yaml
steps:
  - uses: actions/checkout@v7
  - uses: nanoxd/skl/.github/actions/setup-skl@main
    with:
      version: v0.1.0
  - run: skl sync --locked
```

Pin the action itself to a reviewed full commit SHA for reproducible workflows.
The required `version` is an exact release tag; `latest` is deliberately rejected.
The action downloads the matching macOS or glibc Linux binary for ARM64 or x86_64,
verifies its release SHA-256 sidecar, and adds it to `PATH`. It requires `git` and
`tar` for subsequent skill operations, and `curl` and `shasum` for installation;
GitHub-hosted macOS and Ubuntu runners include these tools. It does not sync any
skills or edit configuration. Provision your manifest and lockfile before running
`sync`, for example by checking out your dotfiles and setting `SKL_CONFIG_DIR`.

The checksum detects corrupt downloads; it is published alongside the binary,
not an independent signature. No GitHub token or repository write permission is
needed to install public releases.

## Quick start

On the first machine, add skills:

```sh
skl add example/notes
skl add example/agent-skills --skill review
skl add ~/dev/skills/my-skill
```

Each `add` edits `~/.config/skl/skills.toml`, resolves the skill to a commit in
`skills.lock`, and links it into `~/.claude/skills` and `~/.agents/skills`. Commit
both files to your dotfiles.

On every other machine, pull your dotfiles and run:

```sh
skl sync
```

## Project scope

Use `--project PATH` to share skills with a team through a repository:

```sh
skl --project . add example/agent-skills --skill review
skl --project . add ./skills/my-skill
skl --project . sync --locked
```

The project directory must already exist. Commit `skills.toml` and `skills.lock`
at its root. Local sources inside the project are written as relative paths and
resolve against that root on every machine. The default agents install into
`<project>/.claude/skills` and `<project>/.agents/skills`. Other named agents use
their project directories; `skl --project . agents list` shows the full mapping.
Project-only harnesses Eve and PromptScript are also supported. Custom agent
paths starting with `./` resolve against the project root; explicit `~/` and
absolute paths still work and intentionally select those locations.

Scope is always explicit: running `skl sync` inside the repository still uses
the global manifest. `--project` works before or after the subcommand. It ignores
`SKL_CONFIG_DIR` for manifest lookup, and does not search parent directories.
Profiles, ownership state, run locks, backups, git caches, and the content store
are isolated under `<data>/projects/<SHA-256 of canonical project path>`;
`<data>` follows the usual `SKL_DATA_DIR` / XDG settings. Global installs and
separate checkouts never share ownership or garbage collection. Runtime data
stays outside the repository.

Ignore the generated agent skill links in git; teammates recreate them with
`skl --project . sync --locked`. A fresh checkout needs only the manifest, lock,
and any local skill sources. Moving an existing working tree with generated
links keeps those links pointing at the old cache. They are treated as unowned
conflicts at the new root: use `skl --project . sync --locked --force` to back
them up and rebuild them. Old project caches are retained and can be removed
manually once no longer needed.

## Repository groups

Declare a repository once and select several skills by their discovered names:

```toml
[repositories.team]
source = "example/agent-skills"
ref = "main"
skills = ["review", "testing"]
agents = ["codex"]
profiles = ["work"]

[skills]
# Overrides replace this group's defaults for this skill.
review = { repository = "team", ref = "v2.0.0", agents = ["claude-code", "codex"] }
# Aliases also work for skills not included in the group's selection.
notes = { repository = "team", subdir = "skills/notes" }
```

A repository supports `source`, `ref`, `skills`, `agents`, `profiles`, and `os`.
Omit `skills` (or use `[]`) for an alias-only declaration. Its `agents`, `profiles`,
and `os` defaults apply to every selected skill and alias reference; a per-skill
value replaces the corresponding default. `repository` cannot be combined with
`source` or `path`. Edit group declarations in `skills.toml`; `configure` and
`remove` operate on explicit `[skills]` entries.

Use `skills = "*"` to keep tracking a whole repository. Ordinary `sync` and
`update` discover its current membership: newly discovered skills are added,
removed skills are pruned, and existing skills keep their commit pins until
updated. `update NAME` refreshes that skill's pin while still reconciling wildcard
membership. Discovery follows the group's ref; a per-skill ref override only
changes that skill's content. Explicit alias references remain selected even if
the skill disappears from the wildcard membership.

The lock records complete wildcard membership, including an empty repository.
`sync --locked` uses that snapshot without contacting the remote for discovery;
a populated store needs no network. A fresh machine may still fetch pinned
content. Missing coverage, changed source/ref, and changing/removing a wildcard
declaration require an ordinary sync first. Disabled groups retain their lock
and cached content across profiles and operating systems. Read-only commands use
the cached membership and report missing or stale coverage.

Names must be unique across groups, explicit sources, and Xcode imports. To
customize a generated skill, declare the same name with `repository` pointing to
that group. Ambiguous duplicate names inside a wildcard repository are errors;
use an explicit selection and `subdir` instead. Discovery failures and collisions
abort before changing the installation. Failed individual fetches or unpacks
retain their previous pin and links. `--dry-run` discovers in a temporary cache
without publishing lock/state/store/link changes.

Saved update plans support explicit selections and alias references. Wildcard
groups currently require ordinary `update`; plan/apply reject them rather than
silently rediscovering membership after review.

## Manifest

`~/.config/skl/skills.toml`:

```toml
# Agents to link skills into. Defaults to both of these.
agents = ["claude-code", "codex"]

[skills]
# owner/repo on GitHub, following its default branch
notes = "example/notes"

# A branch or tag to follow, and the directory when the repo has several skills
review = { source = "example/agent-skills", ref = "v2.0.0", subdir = "skills/review" }

# A skill you write yourself, linked straight to its directory
my-skill = { path = "~/dev/skills/my-skill" }

# Only for some agents
shell-helper = { source = "example/agent-skills", agents = ["codex"] }

# Only on machines with the personal profile
keyboard = { source = "example/keyboard", profiles = ["personal"] }
```

A skill entry is either a source string or a table with these keys:

| Key        | Meaning                                                                  |
|------------|--------------------------------------------------------------------------|
| `source`   | Git repository to install from                                           |
| `ref`      | Branch, tag or full 40-character commit SHA. Defaults to the remote HEAD |
| `subdir`   | Directory in the repo that holds `SKILL.md`. Found by name when unset    |
| `path`     | A local skill directory, linked in place                                 |
| `agents`   | Agents for this skill, replacing the top-level list                      |
| `profiles` | Non-empty list of profiles for this skill. Unset means every machine     |
| `os`       | Non-empty list of `macos` and/or `linux`. Unset means either OS           |

Set exactly one of `source` or `path`. `ref` and `subdir` only apply to `source`.
A `source` can be:

- `owner/repo`, meaning `https://github.com/owner/repo.git`
- any git URL, such as `https://…`, `ssh://…` or `git@host:owner/repo.git`
- an absolute path to a local repository

A `path` starting with `~` expands to your home directory, and a relative path
resolves against `~/.config/skl`, so a dotfiles repo can keep its own skills next to
the manifest.

Without `subdir`, a skill is found by the `name` in its `SKILL.md` frontmatter, then
by directory name, and a repo with a single skill can be installed under any name.
Copies inside hidden directories, such as a tool-specific mirror, are ignored only
when a visible skill has the same name. Unique hidden skills, including `.curated`
and `.experimental` collections, remain discoverable. If the name is ambiguous, `skl` asks you to set `subdir`.

Skill names may contain letters, digits, `-`, `_` and `.`, and become the directory
name in each agent. Run `skl agents list` to see every supported harness and its
resolved global skills directory. The registry includes Claude Code, Codex, Cursor,
GitHub Copilot, Gemini CLI, OpenCode, Windsurf, Cline, Roo, Kilo, Amp, Goose,
Continue, Droid, Kiro, Pi, and many more, using the global paths from the
[Vercel skills registry](https://github.com/vercel-labs/skills/blob/main/src/agents.ts).
Harnesses that only support project-local skills are excluded.

```bash
skl agents                              # Show the default list
skl agents list                         # List supported harnesses and paths
skl agents set claude-code cursor opencode
skl sync                                # Apply the new selection
skl agents reset                        # Restore Claude Code + Codex
```

`set` writes the top-level `agents` array in `skills.toml`, preserving comments
and per-skill overrides. This default list travels with your manifest in dotfiles.
Changes take effect on the next sync; only links owned by skl are removed.
Without an explicit list, the defaults remain `claude-code` and `codex`.
Each skill's `agents` replaces the default list, and `agents = []` selects none.

Custom directories are also accepted: `skl agents set claude-code ~/my-agent/skills`.
Paths in the manifest's `agents` must start with `~` or `/`. Named harnesses use
standard home directories; Amp, Devin, Goose, OpenCode, Replit and Universal honor
an absolute `XDG_CONFIG_HOME`. For other harness-specific directory overrides, use
an explicit path. Harnesses sharing a skills directory necessarily see the same
skills, even when only one of them is selected.

## Xcode skills

To import skills from the selected Xcode installation, add this separate section
alongside `[skills]` in `skills.toml`:

```toml
[xcode]
skills = "*"
# Optional: restrict agents or machines
# agents = ["codex"]
# profiles = ["mac"]
# os = ["macos"] # default for Xcode imports
```

Run `skl sync`, then `skl list` to see the exported names. To keep only particular
skills, replace `"*"` with their names, for example:

```toml
[xcode]
skills = ["swiftui-specialist", "app-intents-specialist"]
```

Names come from `SKILL.md` frontmatter, falling back to the exported directory
name. They must be unique and cannot overlap names in `[skills]`. Set `skills = []`
or remove `[xcode]` to uninstall the managed Xcode skills. Selection is currently
configured by editing this section; `skl add` and `skl remove` manage `[skills]`.

Xcode imports default to `os = ["macos"]`, so Linux machines skip exporting and
linking them while preserving their lock entries. Xcode must be installed,
selected, and fully set up on the Mac. `skl` runs
`xcrun agent skills export <staging-directory>`, discovers the exported skills,
and publishes selected snapshots into its store before linking them into agents.
It never exports directly into an agent's directory. An existing manual Xcode
export is a conflict; use `--force` to back it up before installing managed links.

Each normal sync refreshes the export. With `"*"`, newly exported skills are added
and removed exports are uninstalled. With named selection, new names are ignored;
a missing or renamed selected skill reports an error and retains its last working
snapshot until you change the selection. Export or validation failures also retain
previous snapshots. An inactive profile skips Xcode entirely.

Xcode may reorder frontmatter fields on each export. `skl` normalizes their order
while preserving multiline values and file permissions, avoiding spurious updates.
The lockfile records each skill's Xcode version/build and SHA-256 content digest.
This is provenance: another machine exporting from another Xcode version may
produce different content and update these entries. Unlike git commits, Xcode
exports cannot be downloaded from the lockfile. `sync --locked` uses only existing
cached snapshots and fails if the selection or cache needs updating. Wildcard coverage
is recorded separately: changing a named subset to `"*"` requires a normal sync
before `--locked` can succeed. Older locks without coverage metadata also require
one normal sync for wildcard imports. It does not
invoke Xcode or check whether the installed Xcode has changed. `--dry-run` describes
the export without invoking it; additions and content changes cannot be previewed
until an export runs. `--patch` remains specific to git skills.

## Lockfile

`~/.config/skl/skills.lock` is generated. It records the source, ref, commit and
directory of every git skill:

```toml
version = 1

[skills.review]
source = "example/agent-skills"
ref = "v2.0.0"
commit = "4f3c2a1e9b…"
subdir = "skills/review"
```

`sync` installs the locked commit until you run `skl update`. Editing a skill's
`source`, `ref` or `subdir` in the manifest makes `sync` resolve it again. Local
`path` skills aren't locked.

## Commands

```sh
skl sync                     # install, link and prune to match the manifest and lock
skl sync --dry-run           # show what would change
skl sync --locked            # fail if the lock doesn't cover the manifest
skl sync --force             # back up anything in the way instead of skipping it
skl sync --patch             # print the full diff of any skill whose commit changes

skl update                   # move every skill to its ref's newest commit
skl update notes             # just one
skl update --dry-run --patch # review an update before applying it

skl add owner/repo                 # a repo with a single skill
skl add owner/repo -s a -s b       # pick skills from a repo with several
skl add owner/repo --all           # every skill in the repo
skl add owner/repo --ref v2.0.0    # follow a branch or tag, or pin a full SHA
skl add ~/dev/skills/my-skill      # a local skill
skl add owner/repo --no-sync       # only edit the manifest
skl add owner/repo --agent codex --only-profile work --os macos
skl configure review --agent codex --only-profile work
skl configure review notes --default-agents --all-os --all-profiles --no-sync

skl remove notes             # remove from the manifest and unlink

skl info review              # provenance, selection rules, content and link ownership
skl doctor                   # diagnose installation problems without changing files
skl list                     # skills, locked commits, missing links, unmanaged skills

skl profile                  # show the active profiles and where they came from
skl profile set personal     # set this machine's profiles
skl profile clear            # remove them
```

`add` and `configure` accept repeatable `--agent AGENT`, `--os macos|linux`,
and `--only-profile NAME`. Each supplied flag replaces that field's entire list;
omitted fields stay unchanged. Agents may be known harness names, `~/...` paths,
or absolute paths. `--only-profile` sets a skill's restrictions, while global
`--profile` selects the profiles used by the ensuing sync.

`configure <names...>` edits existing `[skills]` entries without fetching their
sources to edit them. It syncs afterward unless `--no-sync` is supplied. Use
`--default-agents`, `--all-os`, or `--all-profiles` to remove an override and
restore the corresponding default; each reset conflicts with its setting flag.
Batch edits validate the whole resulting manifest before writing, preserve
comments and unspecified fields, and fail without changing the manifest if any
name is unknown or settings are invalid. Symlinked manifests remain symlinked.
`add` leaves existing entries unchanged, even when settings flags are supplied;
use `configure` to change those entries. These commands do not edit `[xcode]`.

`--profile <name>` works with every command and can be repeated.

`sync` and `list` exit non-zero when something needs attention, such as a conflict, a
skill that failed to fetch, or a missing link.

`skl info <name>` inspects a configured skill or an existing entry in configured
agent directories. It shows manifest source and ref, full pinned commit and
subdirectory (including stale lock entries), local paths or cached Xcode provenance,
profile/OS selection reasons, and actual versus recorded link targets. A link is
owned only when its target still matches the state file. Old recorded destinations
are shown too, even after the configured agents change.

`info` is read-only: it takes no run lock, fetches nothing, invokes no Xcode export,
and creates no files. Exit 0 means the skill was found and inspected, even if it is
excluded, missing content, unlocked, or has conflicts. Unknown names, invalid
arguments, and unreadable or malformed metadata exit non-zero. Xcode wildcard
names become known after a successful sync; explicitly selected names can be
inspected before export. Unmanaged entries can be inspected even without a manifest.

`skl list` ends with a "Not managed by skl" section: every entry in each agent's
skills directory that `skl` doesn't own, such as skills an app installed or leftovers
from another tool, so you can see everything an agent will load.

## Searching the catalog

```sh
skl search swift testing
skl search review --owner anthropics --limit 10
skl search rust --json
```

`search` queries the public [skills.sh catalog](https://skills.sh/) and prints
repository, skill name, install count, and a `skl add` command for each result.
Results are sorted by install count. `--limit` defaults to 20 and accepts 1–100;
`--owner` restricts results to a GitHub owner. Search does not install anything,
read local configuration, require `HOME`, or create files.

Your query, limit, and optional owner are sent to
`https://skills.sh/api/search`. `SKL_SEARCH_URL` can override the complete endpoint
URL for another compatible service or local testing. Requests have a 15-second
timeout and responses are limited to 1 MiB. HTTP failures, malformed responses,
and invalid repository/skill identities produce errors rather than empty results.
Remote identities must be valid before they are displayed or used in suggested
commands.

`--json` uses the same versioned envelope as inspection commands, with
`command: "search"`. Successful `data` contains `query`, nullable `owner`, `limit`,
and `skills`; each skill has `name`, `source`, `installs`, and `install_command`.
Empty results are successful with an empty `skills` array. Runtime errors use
`status: "error"`, `data: null`, and `runtime-error` diagnostics, with no additional
stderr output. Invalid CLI syntax still uses clap's usual stderr errors.

## Browsing repositories

Run `skl discover owner/repo` (or pass a git URL) to see the resolved commit,
all available skill names, their subdirectories, and descriptions before installing.
Use `--ref` to inspect a branch, tag, or full commit SHA:

```sh
skl discover anthropics/skills
skl discover owner/repo --ref v1.0
```

Discovery needs no manifest and uses a temporary repository cache that is removed
when the command finishes. It does not change configuration, installed skills,
links, or the run lock. A root skill has subdirectory `.`. Duplicate names remain
separate rows so their locations are visible. As with `add`, symlinked `SKILL.md`
files are excluded, and hidden-directory copies are omitted only when a visible skill has the same name.
Descriptions support common plain, quoted, and multiline block frontmatter scalars;
missing or unsupported descriptions are blank. Empty repositories with a commit
but no skills produce a successful “No skills found” result.

## Local collections

Import several skills from a local directory without copying their contents:

```sh
skl add ./skills --all
skl add ./skills --skill review --skill testing
```

Collections are searched recursively, including hidden directories; `.git` and
symlinked directories or `SKILL.md` files are skipped. Visible skills take
precedence over hidden copies with the same name. Other duplicate names fail
before the manifest changes; add their individual directories with distinct
`--skill` names to disambiguate. Selections match frontmatter names first, then
directory names. Without selection flags, a collection containing several skills
asks you to choose. `--ref` is only for Git sources.

Each selected directory becomes an ordinary local `path` entry, so edits are
immediately visible to agents and no Git lock entries are created. Paths beneath
the config directory are saved relative to it for portable dotfiles; other paths
use `~/...` when possible. The collection itself is not tracked: rerun `add --all`
to include newly created skills. A directory with its own `SKILL.md` retains the
single-skill behavior, including `--skill NAME` to rename it; use `--all` to scan
its nested skills too. Existing direct local skills may still use a symlinked
`SKILL.md`; collection discovery only accepts regular files.

## Profiles

Some skills only belong on some machines. Give them `profiles`:

```toml
[skills]
keyboard = { source = "example/keyboard", profiles = ["personal"] }
```

Skills without `profiles` go on every machine. A skill with `profiles` is installed
only when one of them is active. The active profiles come from the first of these
that is set:

1. `--profile <name>`, repeatable
2. `SKL_PROFILE`, comma-separated, such as `SKL_PROFILE=personal,work`
3. `skl profile set <name>...`, stored in `~/.local/share/skl/profile`

`--profile` and `SKL_PROFILE` replace the machine setting for that run rather than
adding to it. The machine setting lives in the data directory on purpose, so it stays
out of your dotfiles. `skl profile set` replaces any earlier setting and takes effect
on the next sync. `skl profile` prints the active profiles and where they came from.

A skill outside the active profiles isn't installed, links `skl` made for it are
removed, while its locked files and repository cache remain available for offline
reactivation. Its lock entry is left as it is, and
`skl update` skips it (naming it explicitly is an error), so machines with different
profiles share one `skills.lock` without rewriting each other's entries. `skl list`
shows such skills as `(only for <profiles>)`.

This fits a bootstrap script that already tells machines apart. If it sets
`MACHINE_PROFILE=server` or `MACHINE_PROFILE=personal`, for example:

```sh
skl profile set "$MACHINE_PROFILE"
skl sync
```

## Operating systems

Use `os` to install a skill only on particular operating systems:

```toml
[skills]
mac-tools = { source = "example/mac-tools", os = ["macos"] }
linux-tools = { path = "~/dev/linux-tools", os = ["linux"] }
work-tools = { source = "example/work-tools", os = ["macos"], profiles = ["work"] }
```

Accepted values are `macos` and `linux`. Git and local skills without `os` apply
on either OS; `[xcode]` defaults to `os = ["macos"]`. When both `os` and `profiles`
are specified, both must match. `skl list` labels OS-excluded skills with
`(only on ...)`.

Excluded skills are not fetched, exported, or linked. Previously managed links
are removed using the normal ownership rules, and unused cached files can be
collected, while lock entries remain unchanged for other machines. Explicitly
updating an OS-excluded skill reports an error. Configure these filters in the
manifest; no machine-specific profile setup is required for OS detection.

## Reviewing updates

A skill is a prompt your agent acts on, often with access to your shell and files.
Updating a third-party skill changes what your agent will do, which makes it a
supply-chain change in the same way as bumping a dependency. Read it before you take
it.

Whenever a skill's commit changes, through `skl update` or a manifest edit, `skl`
prints a diffstat of that skill's directory between the old and new commit, or says
its files didn't change. A skill whose `source` changed only notes where it moved
from. Add `--patch` to see the full diff. To review without changing anything:

```sh
skl update --dry-run --patch
```

Then run `skl update` to apply it, or `skl update <name>` to take only the skills you
reviewed. The new commits land in `skills.lock`, so the change shows up in your
dotfiles history too.

## Safety

`skl` only changes what it created.

- **Ownership.** Every link `skl` makes is recorded in `state.toml` with its target.
  A link counts as `skl`'s only while it's recorded and still points where `skl` left
  it, and only those links are repointed or removed. A link you made yourself stays
  yours, even if it points into the store.
- **Conflicts.** If anything else is where a link should go, `sync` reports it and
  leaves it alone.
- **`--force`.** Moves the thing in the way into its own directory under
  `~/.local/share/skl/backup/`, next to an `ORIGINAL_PATH` file that says where it
  came from, then links the skill. Backups never overwrite each other.
- **Failed updates.** If a skill can't be fetched, can't be found at the new commit,
  or its new version is broken, it keeps its previous lock entry, files and links.
  The rest of the sync goes ahead.
- **Offline.** A skill already in the store at its locked commit needs no network,
  so `sync` works offline once a machine is set up.
- **One run at a time.** Commands that change anything take a lock on the data
  directory, and a second run waits for the first to finish.
- **Dotfiles links.** If `skills.toml` or `skills.lock` is a symlink into a dotfiles
  checkout, `skl` writes to its target and keeps the link. Writes go through a
  temporary file, so an interrupted run never leaves half a file.
- **`SKILL.md` must be a regular file.** A symlinked `SKILL.md`, which could point
  anywhere, isn't treated as a skill and is never installed.

Git runs through your own `git` with prompts disabled, so private repos work with the
SSH keys and credential helpers you already have.

## Storage

```
~/.config/skl/
  skills.toml                   the manifest
  skills.lock                   the lockfile

~/.local/share/skl/
  store/<repo>/<commit>/<dir>   each skill's files at its locked commit
  store/xcode/<digest>/<name>  selected Xcode skill snapshots
  repos/                        git caches, so updates only fetch what changed
  state.toml                    the links skl created
  profile                       this machine's profiles
  backup/                       what --force moved aside
  run.lock                      held while a command runs
```

Only each skill's own directory is unpacked into the store, and agent directories
link to it. `<dir>` is the skill's directory in the repo with `/` written as `%2F`,
or `@root` for a skill at the top of the repo. Store commits and repo caches that
nothing uses any more are deleted after each sync.

## Environment

| Variable          | Effect                                          |
|-------------------|-------------------------------------------------|
| `SKL_CONFIG_DIR`  | Config directory, instead of `~/.config/skl`    |
| `SKL_DATA_DIR`    | Data directory, instead of `~/.local/share/skl` |
| `XDG_CONFIG_HOME` | Config directory becomes `$XDG_CONFIG_HOME/skl` |
| `XDG_DATA_HOME`   | Data directory becomes `$XDG_DATA_HOME/skl`     |
| `SKL_PROFILE`     | Active profiles, comma-separated                |

The `SKL_` directory variables take precedence over the `XDG_` ones. Empty values
are treated as unset. XDG directory values must be absolute; relative values are
ignored and the defaults above are used. These rules apply on both Linux and macOS.
Agent skill directories remain at the locations documented above; use a custom
`agents` path to change them.

## Dotfiles integration

Commit `~/.config/skl` with your dotfiles, or link `skills.toml` and `skills.lock`
into it from your dotfiles repo. To bring a machine up to date whenever you pull, add
a post-merge hook to the dotfiles repo:

```sh
#!/bin/sh
# .git/hooks/post-merge
command -v skl >/dev/null && skl sync
```

In a bootstrap script, install `skl` after linking your dotfiles, set the machine's
profile, and sync:

```sh
cargo install --git https://github.com/nanoxd/skl
skl profile set personal
skl sync
```

## Releasing

Keep release notes under `## [Unreleased]` in `CHANGELOG.md`. Push a `v*` tag
(for example, `v0.2.0`) to build and publish a release. The release workflow
moves those notes into `## [0.2.0] - YYYY-MM-DD` using the UTC release date,
leaves an empty `[Unreleased]` section, and uses the entry as GitHub release notes.
An existing entry for the exact version takes precedence. Missing or empty notes
fail the release instead of publishing blank notes.

After publishing, the workflow commits the changelog update to the repository's
default branch. It applies only the changelog change from the tagged snapshot;
the tag stays on its original commit. The Actions token must be allowed to push
to that branch. Conflicting changelog edits or a concurrent push fail the save
step without overwriting branch changes. Resolve any conflict and rerun the
workflow; reruns update the existing release and skip already saved entries.
Pull the automated commit before preparing the next release.

This automates changelog headings, not the package version in `Cargo.toml`.

### Saved update plans

Save exact Git commits and full patches for review, then install those same commits:

```sh
skl update --plan ./skills-update.toml
# Or select skills: skl update my-skill --plan ./skills-update.toml
less ./skills-update.toml
skl update --apply-plan ./skills-update.toml --dry-run
skl update --apply-plan ./skills-update.toml
```

Planning uses temporary repository caches and writes only the requested plan file.
The destination must be a new file in an existing directory outside skl's config,
data, agent skill directories and local skill directories. Existing files and
symlink aliases are never overwritten. `--plan` conflicts with `--dry-run`;
`--apply-plan` accepts `--dry-run` but cannot be combined with skill names or
`--plan`.

Plans contain the manifest and lock baseline, effective profiles, OS, machine
paths, resolved destinations, exact target commits, and full Git patches. Applying
rejects changed context instead of resolving newer branch tips. Selected updates
still reconcile the entire manifest, as ordinary `update` does: missing or stale
lock entries are included in the plan, and local skills and links are reconciled.
Local skill content is not snapshotted. Xcode manifests are not supported by saved
plans; use ordinary `update` for those manifests.

A failed planning run writes no plan. Apply uses the normal ownership checks and
keeps previous installs for skills that fail; successful skills may still update.
After a partial apply, generate a new plan against the new lock baseline. Treat
plans as trusted review artifacts: their checksum catches accidental edits but is
not a signature or authentication mechanism. Keep a plan private if its embedded
manifest contains private repository URLs or other sensitive configuration.

`skl doctor` checks the active profiles and OS for missing skills, invalid cached
`SKILL.md` files, missing or conflicting links, stale ownership records, and
required tools on PATH. It prints remedies and exits with status 1 when problems
are found (0 when healthy). It never changes files, takes a run lock, invokes
external tools, or contacts the network. It checks cached Xcode exports without
exporting new skills. Matching user-created links remain healthy and unowned. Local `SKILL.md`
symlinks are accepted; cached Git/Xcode skill files must be regular files. Diagnostics describe a point-in-time view; rerun after
a concurrent sync finishes.

### JSON inspection

Use `skl list --json`, `skl info NAME --json`, or `skl doctor --json` for scripts.
These commands stay offline and read-only. Stdout contains exactly one JSON
object, including when configuration, profile resolution, or filesystem reads
fail. Runtime errors do not also print human messages to stderr. Invalid command
syntax is handled by clap before execution and still uses stderr.

Every document has `schema_version: 1`, `command`, `status`, `data`, and
`diagnostics`. Status is `ok`, `unhealthy`, or `error`; `error` has `data: null`.
Each diagnostic has a stable `code`, `severity` (currently `error`), nullable
`skill` and `path`, a human-readable `message`, and a `remedy`. Scripts should
match codes rather than message text. File paths are absolute, including symlink
targets resolved relative to their link's parent; paths are not canonicalized.
Paths that cannot be represented as UTF-8 produce a structured runtime error.

- `info` data describes one skill: `name`, `manifest`, `origin` (`git`, `local`,
  `Xcode`, or `unmanaged`), optional `source`, `git_ref`, `subdir`, `locked`, and
  `xcode` provenance, `lock_matches`, `content_path`, `content_available`,
  `profiles`, `active_profiles`, `profile_source`, `os`, `enabled`, `exclusions`,
  `agents`, and `links`. Each link has `path`, optional `target` and
  `recorded_target`, `owned`, and `status`: `missing`, `unmanaged`, `broken`,
  `different_target`, `installed`, or `local_source`.
- `list` data has `skills` (the same reports, sorted by name), `unmanaged`
  (entries sorted by path, each with `agent`, `name`, `path`, `kind`, and nullable
  `target`), `active_profiles`, and `profile_source`. Unmanaged entries can also
  appear as conflicting links in a configured skill report. Hidden entries are
  omitted, as in human output.
- `doctor` data has `active_profiles` and `profile_source`. Its diagnostics use
  `stale-lock`, `missing-tool`, `xcode-not-locked`, `not-locked`, `invalid-cache`,
  `missing-local-skill`, `stale-state`, `missing-link`, `unreadable-link`,
  `outdated-link`, `broken-link`, or `link-conflict`.

Runtime failures use `runtime-error`. List health diagnostics use `not-linked`
(or `xcode-not-locked` for undiscovered wildcard imports). Exit codes match human
output: `info` succeeds when inspection completes, even if content or links are
missing; `doctor` fails when it finds any diagnostic; `list` fails for missing
active links or undiscovered active Xcode wildcard imports. List retains its
existing link-presence criteria; use doctor to check ownership and cache health.
Excluded skills remain in list data but do not cause missing-link failures.

### Source shortcuts

`add` and `discover` accept `owner/repo@skill` and GitHub or GitLab tree URLs:

```sh
skl add vercel-labs/agent-skills@web-design-guidelines
skl add https://github.com/owner/repo/tree/main/skills/review
skl discover https://gitlab.com/group/project/-/tree/main/skills/review
skl add owner/repo --ref main --subdir skills/review
```

Tree paths and `add --subdir` select the exact directory containing `SKILL.md`,
including hidden directories; use `--subdir .` for the repository root. The
manifest stores the repository, ref, and directory separately, so subsequent
syncs and updates use the same selection. `@skill` selects by skill or directory
name and cannot be combined with a different `--skill` or with `--all`.

A tree URL normally uses the first segment after `tree/` as its ref. If an
advertised branch or tag containing slashes also matches the URL, skl asks for
an explicit `--ref` rather than guessing. For example, append
`--ref feature/topic` to a URL ending in `tree/feature/topic/skills/review`.
Explicit refs must match the URL; an explicit directory must agree with its
path. To override either, use the repository URL with `--ref` and `--subdir`.
Tree URLs with percent escapes, fragments, or query strings are rejected with
instructions to use those flags. Ordinary git URLs, SSH credentials and private
repository authentication continue to pass through to git unchanged.

### Interactive installation

Use `skl add owner/repo --interactive` to choose skills and agent destinations
with numbered prompts. Enter comma-separated numbers or `all`; enter `q`, an
empty line, or end-of-input to cancel. Invalid selections stop the command.
Cancellation and invalid input do not write the manifest, links, or shared state.
Skill discovery uses disposable storage while you choose; skl acquires its write
lock and revalidates the selection before editing the manifest.

Explicit `--skill`, `@skill`, or `--all` skips the skill prompt. Explicit `--agent`
skips the agent prompt. Local directories use the same collection discovery as
`add --all`; selecting a nested skill keeps its frontmatter name and exact path,
even when the collection root also contains a skill.
Selected agents become per-skill overrides; existing manifest entries retain their
settings, as with ordinary `add`. Without `--interactive`, behavior remains
noninteractive. The flag requires both stdin and stderr to be terminals; use
explicit flags in scripts.

`skl agents detect` reports recognizable harness configuration directories without
changing files. The interactive agent list marks these hints, but still lets you
choose any known harness. A configuration directory does not prove an agent is
installed, and shared directories such as `~/.agents/skills` do not identify all
the harnesses that might read them. Detection never changes your default agents. With `--project`, detection examines
that project instead of your home directory, and selection includes project-only
harnesses such as Eve. Shared project skill directories do not identify individual
harnesses either.
