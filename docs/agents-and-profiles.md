---
title: Agents and machine selection
---

[Documentation](index.md) · [GitHub](https://github.com/nanoxd/skl)

# Agents and machine selection

Choose where skills are linked and which machines receive them.

[Agents](#agents) · [Profiles](#profiles) · [Operating systems](#operating-systems)

## Agents

Run `skl agents list` to see every supported harness and its
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
on either OS; the Xcode source defaults to `os = ["macos"]`. When both `os` and `profiles`
are specified, both must match. `skl list` labels OS-excluded skills with
`(only on ...)`.

Excluded skills are not fetched, exported, or linked. Previously managed links
are removed using the normal ownership rules, and unused cached files can be
collected, while lock entries remain unchanged for other machines. Explicitly
updating an OS-excluded skill reports an error. Configure these filters in the
manifest; no machine-specific profile setup is required for OS detection.
