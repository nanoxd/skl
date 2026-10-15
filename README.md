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
Copies inside hidden directories, such as a tool-specific mirror, are ignored when the repo has
another copy. If the name is ambiguous, `skl` asks you to set `subdir`.

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

skl remove notes             # remove from the manifest and unlink

skl list                     # skills, locked commits, missing links, unmanaged skills

skl profile                  # show the active profiles and where they came from
skl profile set personal     # set this machine's profiles
skl profile clear            # remove them
```

`--profile <name>` works with every command and can be repeated.

`sync` and `list` exit non-zero when something needs attention, such as a conflict, a
skill that failed to fetch, or a missing link.

`skl list` ends with a "Not managed by skl" section: every entry in each agent's
skills directory that `skl` doesn't own, such as skills an app installed or leftovers
from another tool, so you can see everything an agent will load.

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
removed, and its files leave the store. Its lock entry is left as it is, and
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
