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

[Manifest](#manifest) · [Repository groups](#repository-groups) · [Lockfile](#lockfile)

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
| `repository` | Repository group alias; see [repository groups](#repository-groups) |
| `path`     | A local skill directory, linked in place                                 |
| `agents`   | Agents for this skill, replacing the top-level list                      |
| `profiles` | Non-empty list of profiles for this skill. Unset means every machine     |
| `os`       | Non-empty list of `macos` and/or `linux`. Unset means either OS           |

Set exactly one of `source`, `path`, or `repository`. `repository` refers to a
[repository group](#repository-groups). `ref` and `subdir` apply to Git sources,
including repository references.
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
name in each agent.

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

A repository supports `source`, `ref`, `skills`, `exclude`, `agents`, `profiles`,
and `os`.
Omit `skills` (or use `[]`) for an alias-only declaration. Its `agents`, `profiles`,
and `os` defaults apply to every selected skill and alias reference; a per-skill
value replaces the corresponding default. `repository` cannot be combined with
`source` or `path`. Edit group declarations in `skills.toml`; `configure`
operates on explicit `[skills]` entries.
`remove` also supports cached wildcard members as described below.

Use `skills = "*"` to keep tracking a whole repository. Ordinary `sync` and
`update` discover its current membership: newly discovered skills are added,
removed skills are pruned, and existing skills keep their commit pins until
updated. `update NAME` refreshes that skill's pin while still reconciling wildcard
membership. Discovery follows the group's ref; a per-skill ref override only
changes that skill's content. Explicit alias references remain selected even if
the skill disappears from the wildcard membership.

Use `exclude = ["legacy", "experimental"]` alongside `skills = "*"` to omit
exact discovered names. Names are case-sensitive; patterns are not supported.
An empty array is allowed, and names not yet present in the repository stay
excluded if they appear later. Exclusions are only valid with wildcard selection.
Explicit `[skills]` entries remain selected, including an alias reference to the
same group; exclude a name to resolve a collision with another explicit source.

`skl remove NAME` removes an explicit entry and adds the name to `exclude` in
any wildcard group whose cached membership includes it. Removing a same-group
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

Saved update plans support explicit selections, alias references, and wildcard
groups. Plans snapshot membership and exact pins; applying a plan never
rediscovers membership after review. See [saved update plans](updates.md#saved-update-plans).

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
