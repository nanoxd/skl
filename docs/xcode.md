---
title: Xcode skills
---

[Documentation](index.md) · [GitHub](https://github.com/nanoxd/skl)

# Xcode skills

Import the skills that ship with your selected Xcode on macOS.

## Xcode skills

Xcode is an [app source](app-skills.md#app-sources) that skl knows how to export
from. Declare it like any other source in `skills.toml`:

```toml
[sources.xcode]
app = "xcode"
skills = "*"
# Optional: leave out skills, restrict agents or machines
# exclude = ["device-interaction"]
# agents = ["codex"]
# profiles = ["mac"]
# os = ["macos"] # default for Xcode
```

Run `skl sync`, then `skl list` to see the exported names. To keep only particular
skills, replace `"*"` with their names, for example:

```toml
[sources.xcode]
app = "xcode"
skills = ["swiftui-specialist", "app-intents-specialist"]
```

The shorter `[xcode]` table from earlier versions still works and means the same
thing. It accepts `skills`, `exclude`, `agents`, `profiles`, and `os`. Declare Xcode
once, with either form.

Names come from `SKILL.md` frontmatter, falling back to the exported directory
name. They must be unique and cannot overlap other names in the manifest. Like any
member of a declared source, an Xcode skill can have its own `agents`, `profiles`,
or `os` with an entry such as `swiftui-specialist = { source = "xcode", agents =
["codex"] }`, and `skl configure` writes that entry for you. `skl remove` takes a
name out of the list, or adds it to `exclude` when the selection is `"*"`. Set
`skills = []` or remove the declaration to uninstall the managed Xcode skills.

## Requirements

Xcode defaults to `os = ["macos"]`, so Linux machines skip exporting and linking
while preserving the lock entries. On a Mac, skl first runs `xcrun --find
xcodebuild`. If that fails, as it does with only the Command Line Tools installed,
sync prints `xcode: app unavailable; skipping`, removes the Xcode links it owns,
and keeps the lock entries and cached content for other machines. This is the same
as any app whose command isn't installed.

With Xcode installed, skl runs `xcrun agent skills export <staging-directory>`
with the same limits as other app commands. The command gets a temporary `HOME`
and a minimal environment, keeping only `DEVELOPER_DIR` so a non-default Xcode
stays selected. The export must finish within 30 seconds by default. Set `timeout`
on `[sources.xcode]` to allow up to 300. skl discovers the exported skills
and publishes the selected ones into its store before linking them into agents. It
never exports directly into an agent's directory. An existing manual Xcode export
is a conflict, and `--force` backs it up before installing managed links.

## Updates and the lockfile

Each normal sync refreshes the export. With `"*"`, newly exported skills are added
and removed exports are uninstalled. With named selection, new names are ignored.
A missing or renamed selected skill reports an error and keeps its last working
snapshot until you change the selection. Export or validation failures also keep
previous snapshots. An inactive profile skips Xcode entirely.

Xcode may reorder frontmatter fields on each export. skl normalizes their order
while preserving multiline values and file permissions, avoiding spurious updates.
The lockfile records the export command and the Xcode version and build once
under `[sources.xcode]`, with each skill's SHA-256 content digest under
`[sources.xcode.snapshots]`. A `"*"` selection also records the complete export
under `[sources.xcode.members]`.

The lock entries are provenance. Another machine exporting from another Xcode
version may produce different content and update them, and unlike git commits,
Xcode exports cannot be downloaded from the lockfile. `sync --locked` uses only
existing cached snapshots, without invoking or probing Xcode, and fails if the
selection or cache needs updating. Changing a named subset to `"*"` needs one normal
sync before `--locked` can succeed. `--dry-run` describes the export without running
it, so additions and content changes cannot be previewed until an export runs.
`--patch` remains specific to git skills.

Version 2 locks kept Xcode skills in an `[xcode]` table and their content under
`store/xcode`. skl reads them as members of the `xcode` source, `--locked` keeps
using the old cache without rewriting the lock, and the next normal sync moves the
content under `store/commands` and writes version 3.
