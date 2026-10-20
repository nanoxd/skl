---
title: App-provided skills
---

[Documentation](index.md) · [GitHub](https://github.com/nanoxd/skl)

# App-provided skills

Keep skills aligned with the app installed on each machine. Review the
[command trust model](storage-and-safety.md#trusting-app-commands) before adding
commands to dotfiles or project manifests.

## CLI output

A command array is an executable followed by its arguments. Its stdout becomes
the skill's `SKILL.md`:

```toml
[skills]
cmux = { command = ["cmux", "--skill"], version-command = ["cmux", "--version"], profiles = ["personal"], os = ["macos"] }
orca-cli = { command = ["orca", "skills", "get", "orca-cli"], os = ["macos"] }
```

`orca skills get` asks the installed CLI for its version-matched guide, avoiding
a manually bumped release tag or a newer Git HEAD. `version-command` is optional;
when configured, its trimmed UTF-8 stdout is recorded as provenance. A failed
version probe fails the import and preserves the previous installation.

`sync` reruns all enabled imports. `update` reruns all imports, or just the named
skills with `update NAME`. Agents, profiles, and OS filters work as for Git skills.
Missing executables print an unavailable note and remove owned links, while
retaining the lock and snapshot for other machines. Nonzero exits, timeouts,
oversized output, and invalid exports report errors and keep the working install.

Commands run without the real HOME or inherited app environment. An app whose
export command needs its normal configuration may require a trusted adapter;
test the import on the machine where the app is installed.

## Directory exports

For a CLI that exports a skill directory, include `{output}` as a complete
argument:

```toml
[skills]
app-guide = { command = ["my-app", "export-skill", "{output}"], timeout = 60 }
```

This is a syntax example; use your app's actual export arguments. The command
must create the supplied directory with `SKILL.md` directly inside it. References
and scripts in that directory are included. Symlinks and special files are
rejected. One entry imports one skill. For a CLI that exports several, declare an
[app source](#app-sources).

## App sources

A [declared source](configuration.md#declared-sources) can get its skills from an
app's export command instead of Git. The command writes a directory of skills into
`{output}`, and the source selects from them exactly as it would from a Git
repository:

```toml
[sources.tools]
command = ["my-app", "export-skills", "{output}"]
version-command = ["my-app", "--version"]
skills = "*"
exclude = ["experimental"]
os = ["macos"]

[skills]
# A member can have its own agents, profiles, or OS.
reviewer = { source = "tools", agents = ["codex"] }
```

Each skill directory in the export needs a regular `SKILL.md`, and names come from
its frontmatter, falling back to the directory name. `skills` takes `"*"` or a
list, `exclude` works with `"*"`, and `timeout` applies to each command as for
single imports. `remove` and `configure` edit members the same way as Git source
members, and `add` only joins Git sources. A member can override only `agents`,
`profiles`, and `os`.

`app = "xcode"` declares a source from a known app instead of a command. skl
supplies the command, the version probe, a default `os`, and a check that the app
is installed. See [Xcode skills](xcode.md).

A missing executable prints an unavailable note, removes the members' owned links,
and keeps their lock entries and snapshots, as for a single import. The lock
records the command and app version once under `[sources.<alias>]`, with one
content digest per member, and a `"*"` selection also records the complete export,
so `sync --locked` can check that every member is cached. See the
[lockfile](configuration.md#lockfile) for an example.

## Bundled files

Use an optional local path for skills shipped inside an app or formula:

```toml
[skills]
cmux-cua = { path = "/Applications/cmux.app/Contents/Resources/cmux-cua", optional = true, os = ["macos"] }
rocketsim = { path = "/Applications/RocketSim.app/Contents/Resources/Agent-Skill/rocketsim", optional = true, os = ["macos"] }
xcodebuildmcp-cli = { path = "/opt/homebrew/opt/xcodebuildmcp/libexec/skills/xcodebuildmcp-cli", optional = true, os = ["macos"] }
```

These link directly to the installed files, so app upgrades are reflected
immediately. They have no content lock or version probe. If the directory is
absent, sync removes owned links and succeeds; an existing directory without a
valid skill still reports an error. Ordinary paths retain their existing
missing-file error behavior. Adjust application and Homebrew paths for the machine.

## Review and reproducibility

Command snapshots record the argv, timeout, optional version command, version
output, and SHA-256 digest in the lockfile. Unchanged content and provenance
leave the lock untouched. Frontmatter field ordering is normalized for every
app snapshot. Changed imports show a diffstat, or a full diff with `--patch`.

`sync --locked` validates and uses the matching cached snapshot without invoking
the app or probing its installation. Missing or stale caches require a normal
sync. A lock cannot download an app snapshot onto another machine.

`--dry-run` never runs commands, even with `--patch`: arbitrary commands cannot
promise a side-effect-free preview. It describes deferred imports instead.
Saved update plans reject command imports and app sources with a `command`,
so use normal sync/update for those. Built-in apps such as Xcode are supported in plans.
`list`, `info`, `doctor`, and `verify` inspect existing data without running apps.

`skl configure NAME` can edit agents, profiles, and OS filters without changing
argv. Its automatic sync defers commands; run `skl sync` explicitly afterward.
