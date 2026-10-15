---
title: Xcode skills
---

[Documentation](index.md) · [GitHub](https://github.com/nanoxd/skl)

# Xcode skills

Import skills from your selected Xcode installation on macOS.

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
