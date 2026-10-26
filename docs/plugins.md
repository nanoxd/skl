---
title: Plugins
---

[Documentation](index.md) · [GitHub](https://github.com/nanoxd/skl)

# Plugins

A plugin bundles skills with the agents, commands, hooks, and MCP servers they rely
on. Installing only a plugin's skills leaves the rest behind, and a skill that
hands work to the plugin's agent then can't find it. skl installs the whole plugin
in Claude Code, Codex, and Cursor, pinned to a commit in `skills.lock` like any
skill.

[Adding a plugin](#adding-a-plugin) · [How it works](#how-it-works) · [Cursor](#cursor) · [One plugin, several copies](#one-plugin-several-copies) · [Inspecting plugins](#inspecting-plugins) · [Ownership and safety](#ownership-and-safety)

## Adding a plugin

```sh
skl add cursor/plugins --plugin pstack
```

`add` finds the plugin through the repository's marketplace file, checks that every
agent can install it, and writes:

```toml
[plugins.pstack]
source = "cursor/plugins"
```

Then sync installs it in each agent. Restart Claude Code (or run `/reload-plugins`)
and Codex, or reload the Cursor window, to load it. `skl remove pstack` uninstalls
it again.

A plugin entry takes these keys:

| Key | Meaning |
|-----|---------|
| `source` | Git repository, as for a skill |
| `ref` | Branch, tag or full commit SHA. Defaults to the remote HEAD |
| `plugin` | The plugin's name in the repository's marketplace. Defaults to the entry's name |
| `subdir` | The plugin's folder, for every agent or per agent. Found through the marketplace when unset |
| `agents` | Any of `claude-code`, `codex`, and `cursor`. Defaults to the manifest's agents that install plugins |
| `profiles`, `os` | As for skills |

`add --plugin` accepts `--ref`, `--subdir`, `--agent`, `--os`, and
`--only-profile`, and `--alias NAME` names the entry when it should differ from the
plugin's name. A plugin's own name, not the entry's, is what each agent shows, as
`pstack@skl`. An entry can't share its name with a skill, since `update`,
`remove`, and `info` take either.

When `add` installs skills that belong to a plugin shipping more than skills, it
warns and prints the `skl add --plugin` command that installs the whole plugin.
`skl discover` notes the plugin too.

## How it works

skl unpacks the plugin's folder at its locked commit into the store, the same way
it unpacks a skill. It then generates a marketplace named `skl` for each agent,
under `~/.local/share/skl/plugins/<agent>/`, with one symlink per plugin into the
store, and installs each plugin through the agent's own CLI:

| | Claude Code | Codex | Cursor |
|---|---|---|---|
| Register skl's marketplace, once | `claude plugin marketplace add` | `codex plugin marketplace add` | none |
| Install | `claude plugin install NAME@skl` | `codex plugin add NAME@skl` | copies it into `~/.cursor/plugins/local` |
| New commit | repoints the symlink | repoints the symlink and adds the plugin again | replaces the copy |
| Uninstall | `claude plugin uninstall NAME@skl` | `codex plugin remove NAME@skl` | removes the copy |

Claude Code reads a local marketplace's plugins in place, so an update only moves
the symlink. Codex keeps its own copy, so skl adds the plugin again. The agents
record the marketplace and enabled plugins in their own settings. skl never edits
`settings.json` or `config.toml` itself.

`sync` installs the locked commit, `update` moves it and shows what changed, and
`sync --locked` fails rather than changing the lock. A dry run prints what it would
install, update or uninstall and runs no agent CLI. A [saved update
plan](updates.md) pins each plugin's commit and includes the full diff of every
folder it installs.

## Cursor

Cursor has no command for installing a plugin, and it skips a symlink in its
local plugin folder that points elsewhere on disk. So skl copies the plugin's
folder into `~/.cursor/plugins/local/<name>` and records a digest of the copy.
Cursor finds its copy through `.cursor-plugin/marketplace.json`, and reads
`.cursor-plugin/plugin.json` or a root `plugin.json`.

A new commit replaces the copy, staged beside the local folder so Cursor never
loads half of one. skl replaces or removes a copy only while it still has the
digest skl recorded. Once you edit it, it's yours: skl reports a conflict, leaves
it in place, and stops managing it. Move it aside and sync to get skl's copy back.
A folder of the same name that skl didn't make is a conflict too. When a plugin of
the same name is also installed from Cursor's marketplace, Cursor loads that one
instead.

skl installs for Cursor only when `~/.cursor` exists. On Enterprise plans, local
plugins need **Allow Local Plugin Imports** turned on by an admin.

## One plugin, several copies

Some repositories ship a separate copy of a plugin for each agent. cursor/plugins
keeps pstack for Cursor in `pstack/` and an adapted copy for Claude Code in
`harness/pstack/claude-code/`. Each agent finds its copy through the marketplace
file it reads: Claude Code reads `.claude-plugin/marketplace.json`, and Codex reads
`.agents/plugins/marketplace.json` first, then the Claude Code one. Override the
folder for every agent with a string, or per agent with a table:

```toml
[plugins.pstack]
source = "cursor/plugins"
subdir = { codex = "harness/pstack/codex" }
```

The lock records the commit once and the folder each agent installs. Changing a
folder installs that agent's copy at the same commit.

## Inspecting plugins

`skl list` shows each plugin's commit and any agent it isn't installed in yet.
`skl info pstack` shows the pinned commit, each agent's folder and store copy, and
what skl recorded installing. `skl doctor` reports plugins that aren't installed
or are out of date, missing marketplace links, edited Cursor copies, and installs
left over after an agent was dropped. `skl verify pstack` compares each store
folder, and each Cursor copy, with the locked commit. Codex keeps its own copy
where skl can't see it, so verify checks the store folder Codex installed from.

None of these run an agent's CLI. They read the manifest, the lock, and
`plugins/state.toml`, so they show what skl installed, not what an agent has
loaded.

## Ownership and safety

- skl records each install in `~/.local/share/skl/plugins/state.toml` before
  making it, and only updates or uninstalls plugins it recorded. Plugins you
  installed yourself are left alone.
- The same plugin installed from another marketplace, such as
  `pstack@cursor-plugins`, would load twice. skl reports it as a conflict and
  doesn't install its copy in that agent.
- A missing `claude` or `codex` command, or a machine without `~/.cursor`, skips
  that agent and keeps its installs and the lock entry, like a Mac without Xcode.
- A Cursor copy is skl's only while it matches the digest skl recorded.
- A plugin that fails to fetch keeps its previous commit. A failed install is
  reported and retried by the next sync.
- A lock with plugins is version 4, so an older skl refuses it rather than dropping
  the plugins when it rewrites the lock. Without plugins the lock stays version 3.
- Plugins install for your user account, so a [project](projects.md) manifest
  can't declare them.
