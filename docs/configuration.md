---
title: Configuration
---

[Documentation](index.md) · [GitHub](https://github.com/nanoxd/skl)

# Configuration

`skills.toml` is the source of truth for your skill setup. Edit it in your editor
and run `skl sync` to apply your changes. The `add`, `configure`, and `remove`
commands edit the same file, preserving comments and formatting, then sync by
default. Commit the manifest alongside the generated `skills.lock` so other
machines can install the pinned Git versions.

[Manifest](#manifest) · [Declared sources](#declared-sources) · [Plugins](#plugins) · [Lockfile](#lockfile)

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
| `source`   | Git repository to install from, or a [declared source](#declared-sources) alias |
| `ref`      | Branch, tag or full 40-character commit SHA. Defaults to the remote HEAD |
| `subdir`   | Directory in the repo that holds `SKILL.md`. Found by name when unset    |
| `repository` | Earlier key for a declared source alias, still accepted in place of `source` |
| `command`  | Argv array producing a skill; see [app-provided skills](app-skills.md) |
| `version-command` | Optional argv array recording the installed app version |
| `timeout` | Command deadline in seconds, 1–300; default 30 per invocation |
| `optional` | For `path` only: skip and unlink owned links when the directory is absent |
| `path`     | A local skill directory, linked in place                                 |
| `agents`   | Agents for this skill, replacing the top-level list                      |
| `profiles` | Non-empty list of profiles for this skill. Unset means every machine     |
| `os`       | Non-empty list of `macos` and/or `linux`. Unset means either OS           |

Set exactly one of `source`, `path`, or `command`. `ref` and `subdir` apply to Git
sources, including references to a declared Git source.
A `source` can be:

- `owner/repo`, meaning `https://github.com/owner/repo.git`
- any git URL, such as `https://…`, `ssh://…` or `git@host:owner/repo.git`
- an absolute path to a local repository
- a bare name without `/` or `:`, such as `team`, naming a [declared source](#declared-sources)

A bare name that matches no declared source is an error.

A `path` starting with `~` expands to your home directory, and a relative path
resolves against `~/.config/skl`, so a dotfiles repo can keep its own skills next to
the manifest.

Without `subdir`, a skill is found by the `name` in its `SKILL.md` frontmatter, then
by directory name, and a repo with a single skill can be installed under any name.
Copies inside hidden directories, such as a tool-specific mirror, are ignored only
when a visible skill has the same name. Unique hidden skills, including `.curated`
and `.experimental` collections, remain discoverable. If the name is ambiguous, `skl`
lists each copy and asks you to set `subdir`. A skill from a
[declared source with a `subdir`](#limiting-a-source-to-one-directory) is looked up
only inside that directory.

Skill names may contain letters, digits, `-`, `_` and `.`, and become the directory
name in each agent.

## Declared sources

Declare a source once and select several skills by their discovered names:

```toml
[sources.team]
source = "example/agent-skills"
ref = "main"
skills = ["testing"]
agents = ["codex"]
profiles = ["work"]

[skills]
# A member with its own settings is declared here instead of in the list.
review = { source = "team", ref = "v2.0.0", agents = ["claude-code", "codex"] }
# Aliases also work for skills not included in the source's selection.
notes = { source = "team", subdir = "skills/notes" }
```

A declared source gets its skills from one of three places. `source` names a Git
repository, `command` names an app's export command, and `app` names an app skl
knows, such as `app = "xcode"`. See [app sources](app-skills.md#app-sources)
and [Xcode skills](xcode.md) for the last two. Every declared source also supports
`skills`, `exclude`, `agents`, `profiles`, and `os`, and a Git source supports
`ref` and [`subdir`](#limiting-a-source-to-one-directory). The rest of this section
describes Git sources, and app sources select and override their skills the same way.

Earlier manifests can keep their `[repositories.<alias>]` tables and
`repository = "alias"` references, which mean the same as `[sources.<alias>]` and
`source = "alias"`. Each alias is declared once across `[sources]`,
`[repositories]`, and the [`[xcode]` shorthand](xcode.md).

Omit `skills` (or use `[]`) for an alias-only declaration. Its `agents`, `profiles`,
and `os` defaults apply to every selected skill and alias reference; a per-skill
value replaces the corresponding default. A reference without settings can use the
string form, such as `notes = "team"`. Edit source declarations themselves in
`skills.toml`.

Like `cargo add` with a workspace dependency, `skl add` puts a skill from a Git
repository that a declared source names into that source rather than repeating the
location. GitHub `owner/repo`, HTTPS, and SSH spellings of a repository match each
other, and other locations must be written the same way.

- A source with a `skills` list, even an empty one, gains the name, in order when
  the list is sorted.
- A source without `skills` gains a `name = "alias"` entry.
- A wildcard source drops the name from `exclude`, or reports it as already selected.
  A skill added by a directory name that differs from its frontmatter name gets a
  `name = "alias"` entry, since wildcards select frontmatter names.

Without `--ref`, the skill inherits the source's ref and must exist at it. A tree
URL's ref joins a source without a ref when it names the default branch. Anything
the source doesn't already provide becomes an override entry: a different `--ref`,
`--agent`, `--os`, or `--only-profile` value, or a `subdir` that name lookup would
not find. Each name appears once: a member with an override is declared only by
its entry, not also listed.
`add` names any `agents`, `os`, or `profiles` defaults the skill inherits, since a
source restricted to a profile or OS also restricts where its new members install.
Entries added to a sorted `[skills]` table also keep it sorted.

`configure NAME` on a source member writes the same kind of override, moving a
listed name out of the list, and resetting its last override moves it back. A
wildcard member drops its entry, since the wildcard selects it again, and any
other member returns to the string form. `remove
NAME` takes the name out of source lists along with any override, and also supports
cached wildcard members as described below. A name that is both listed and given a
`source` entry is still accepted, with the entry taking precedence, and
`configure` settles it into one place.

Use `skills = "*"` to keep tracking a whole repository. Ordinary `sync` and
`update` discover its current membership: newly discovered skills are added,
removed skills are pruned, and existing skills keep their commit pins until
updated. `update NAME` refreshes that skill's pin while still reconciling wildcard
membership. Discovery follows the source's ref; a per-skill ref override only
changes that skill's content. Explicit alias references remain selected even if
the skill disappears from the wildcard membership.

Use `exclude = ["legacy", "experimental"]` alongside `skills = "*"` to omit
exact discovered names. Names are case-sensitive; patterns are not supported.
An empty array is allowed, and names not yet present in the repository stay
excluded if they appear later. Exclusions are only valid with wildcard selection.
Explicit `[skills]` entries remain selected, including an alias reference to the
same declared source; exclude a name to resolve a collision with another explicit source.

`skl remove NAME` removes an explicit entry and adds the name to `exclude` in
any wildcard source whose cached membership includes it. Removing a same-source
explicit override also adds an exclusion, even before discovery. This prevents
the wildcard from immediately restoring the skill. Run `sync` first to remove a
wildcard member that has never been discovered. All requested removals are
validated before the manifest is written; comments and dotfile symlinks survive.
Remove the exclusion manually to select the skill again.

The lock records complete wildcard membership, including an empty repository.
`sync --locked` uses that snapshot without contacting the remote for discovery;
a populated store needs no network. Exclusions filter this complete snapshot,
without discarding discovery coverage. An exclusion change that adds or removes
a skill lock entry requires an ordinary sync first; `--locked` never rewrites
the lock. A fresh machine may still fetch pinned content. Missing coverage, changed source/ref, and changing/removing a wildcard
declaration require an ordinary sync first. Disabled sources retain their lock
and cached content across profiles and operating systems. Read-only commands use
the cached membership and report missing or stale coverage.

Names must be unique across declared sources and explicit entries. To
customize a generated skill, declare the same name with `source` set to that
source's alias. Ambiguous duplicate names inside a wildcard source are errors
that list each copy; set the source's `subdir` to the collection you want, or use
an explicit selection with a per-skill `subdir`. See [name clashes](#name-clashes)
for names that two declarations share. Discovery failures and collisions
abort before changing the installation. Failed individual fetches or unpacks
retain their previous pin and links. `--dry-run` discovers in a temporary cache
without publishing lock/state/store/link changes.

Saved update plans support explicit selections, alias references, and wildcard
sources. Plans snapshot membership and exact pins; applying a plan never
rediscovers membership after review. See [saved update plans](updates.md#saved-update-plans).

### Limiting a source to one directory

Some repositories ship the same skills more than once, such as a plugin repository
with one copy for Cursor and an adapted copy for Claude Code in a sibling folder.
Every name is then ambiguous across the whole repository. Set `subdir` on the
declared source to the folder that holds the copy you want:

```toml
[sources.pstack]
source = "cursor/plugins"
subdir = "harness/pstack/claude-code/skills"
skills = "*"
exclude = ["teach"]
```

`subdir` on a declared source limits everything that finds skills by name:
wildcard discovery, names in a `skills` list, and alias references such as
`review = "team"`. Skills in nested folders beneath it are found too. The
directory must stay inside the repository, and `"."` means the whole repository.
A member can still set its own `subdir`, which is the exact directory of its
`SKILL.md` relative to the repository root, even outside the source's `subdir`.
App sources don't take `subdir`.

The lock records the `subdir` alongside the wildcard's membership. Changing it
needs an ordinary `sync`, which rediscovers the membership and looks up any locked
skill again whose directory falls outside the new `subdir`.

`skl add` writes this declaration for you from a tree URL, or a `--subdir`, that
names a folder without a `SKILL.md` of its own. See
[collections](adding-skills.md#collections-in-a-repository).

### Name clashes

Two declarations can provide the same skill name, such as a wildcard source and a
`[skills]` entry, or two wildcard sources. Sync then stops before changing
anything and names both declarations, with the line that keeps each copy:

```text
error: skills `tdd`, `teach` from source `pstack` (cursor/plugins) conflict with another declaration, source `matt` (mattpocock/skills). Keep one copy:
  keep source `matt`'s: add exclude = ["tdd", "teach"] to source `pstack`
  keep source `pstack`'s: add exclude = ["tdd", "teach"] to source `matt`
```

Every name the same two declarations share is reported together. skl doesn't
rename skills at install time. Skills in one collection often refer to each other
by folder name, such as `../teach/SKILL.md`, and a rename would break those
references, so choose one copy with `exclude` instead.

## Plugins

`[plugins.<name>]` installs a whole Claude Code, Codex, or Cursor plugin, with its
agents, commands, hooks, and MCP servers:

```toml
[plugins.pstack]
source = "cursor/plugins"
# The plugin's folder, for every agent or per agent. Found through the
# repository's marketplace file when unset.
subdir = { codex = "harness/pstack/codex" }
```

| Key | Meaning |
|-----|---------|
| `source` | Git repository, as for a skill |
| `ref` | Branch, tag or full commit SHA. Defaults to the remote HEAD |
| `plugin` | The plugin's name in the repository's marketplace. Defaults to the entry's name |
| `subdir` | A folder for every agent, or a table of `claude-code` and `codex` folders |
| `agents` | Any of `claude-code`, `codex`, and `cursor`. Defaults to the manifest's agents that install plugins |
| `profiles`, `os` | As for skills |

See [plugins](plugins.md) for how skl installs them and what it owns.

## Lockfile

`~/.config/skl/skills.lock` is generated. It records the source, ref, commit and
directory of every Git skill, and what each app source last exported:

```toml
version = 3

[skills.review]
source = "example/agent-skills"
ref = "v2.0.0"
commit = "4f3c2a1e9b…"
subdir = "skills/review"

[sources.xcode]
command = ["xcrun", "agent", "skills", "export", "{output}"]
version_command = ["xcrun", "xcodebuild", "-version"]
timeout = 30
version = """
Xcode 27.1
Build version 27A9275"""

[sources.xcode.members]
swiftui-specialist = "swiftui-specialist"

[sources.xcode.snapshots]
swiftui-specialist = "58ac7617114bb700219c9cb16a5e1b334e71d8e353691bc7a98d37a6150c9a01"
```

`sync` installs the locked commit until you run `skl update`. Editing a skill's
`source`, `ref` or `subdir` in the manifest makes `sync` resolve it again. Local
`path` skills aren't locked.

Command entries use the same agents, profiles, and OS settings. `skl configure`
edits these settings while preserving command argv; edit commands themselves in
the manifest. Only explicit `sync` and `update` execute manifest commands.
Automatic sync after other editing commands defers imports until that explicit run.

Each `[sources.<alias>]` table records what a declared source last produced. A Git
wildcard source records its `source`, `ref`, `subdir` if it has one, and complete
`members`, mapping each name to its directory. An app source records its command and app version once,
with one content digest per skill under `snapshots`. A skill exported by a
different app version than the rest, such as after `skl update NAME` on one
member, becomes a small table with its own `version`. A `"*"` app source also
records its complete export under `members`, as in the example. Single-skill
`command` entries keep their own `[commands.<name>]` tables.

A plugin's `[plugins.<name>]` table records its `source`, `ref`, marketplace
`plugin` name, `commit`, and the folder each agent installs under `subdirs`:

```toml
[plugins.pstack]
source = "cursor/plugins"
plugin = "pstack"
commit = "9580c23696ac8d7abed0c166f2f9918800afb819"

[plugins.pstack.subdirs]
claude-code = "harness/pstack/claude-code"
codex = "harness/pstack/claude-code"
```

A lock with plugins is version 4, so an older skl refuses it instead of dropping
the plugins when it rewrites the lock. Otherwise this layout is lock version 3. skl still reads version 1 and 2 locks, including
the `[repositories]` and `[xcode]` tables they used, and migrates them the next
time it writes the lock. `sync --locked` never rewrites an older lock. Command
snapshots are local provenance rather than downloadable pins, so another machine
needs a normal sync first.
