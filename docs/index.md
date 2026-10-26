---
title: skl documentation
---

# skl

<img src="assets/skl-mascot.png" alt="A playful skull mascot waving, carrying a backpack of skill cards" width="180">

File-based skill management for coding agents. Declare your skills in
`skills.toml`, commit it with `skills.lock`, and run `skl sync` to install them
across agents and machines.

[Get started](getting-started.md) · [Download a release](https://github.com/nanoxd/skl/releases) · [GitHub](https://github.com/nanoxd/skl)

## How it works

Your skill setup lives in two files you can review in Git and share:

| File | Purpose |
| --- | --- |
| `skills.toml` | You edit this manifest to choose skills, sources, and agent destinations. |
| `skills.lock` | `skl` generates this lockfile to pin Git skills to exact commits. Commit it alongside the manifest. |

Run `skl sync` to apply the files: it fetches skills, links them into your agents,
and removes links it owns for skills you deselect. `skl add`, `skl configure`,
and `skl remove` edit the manifest and sync for you; you can also edit it directly
in your editor.

Keep the files in your dotfiles for your machines, or at a
[project's root](projects.md) for your team. Restore them on another machine and
sync to install the pinned Git versions. Caches and link ownership state stay
on each machine; see [storage and safety](storage-and-safety.md).

## Guides

| I want to… | Read |
| --- | --- |
| Install skl and set up another machine | [Getting started](getting-started.md) |
| Find and install skills | [Finding and adding skills](adding-skills.md) |
| Install a Claude Code or Codex plugin | [Plugins](plugins.md) |
| Edit the manifest or select a whole repository | [Configuration](configuration.md) |
| Choose agents, profiles, or operating systems | [Agents and machine selection](agents-and-profiles.md) |
| Share skills with a team | [Project skills](projects.md) |
| Review changes before updating | [Reviewing updates](updates.md) |
| Follow installed app skills | [App-provided skills](app-skills.md) |
| Import Apple's skills | [Xcode skills](xcode.md) |
| Install skl in CI | [GitHub Actions](github-actions.md) |

## Reference

- [Commands](commands.md)
- [Diagnostics and JSON output](diagnostics.md)
- [Storage, environment variables, and safety](storage-and-safety.md)
- [Development, releases, and publishing these docs](maintaining.md)
