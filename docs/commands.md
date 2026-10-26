---
title: Command reference
---

[Documentation](index.md) · [GitHub](https://github.com/nanoxd/skl)

# Command reference

Run `skl --help` or `skl <command> --help` for all flags. See also
[adding skills](adding-skills.md), [updates](updates.md), and
[diagnostics](diagnostics.md).

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
skl add owner/repo --subdir skills/set --all  # every skill in one folder, as a source
skl add owner/repo --plugin name   # a whole plugin, in Claude Code, Codex and Cursor
skl add owner/repo --ref v2.0.0    # follow a branch or tag, or pin a full SHA
skl add ~/dev/skills/my-skill      # a local skill
skl add owner/repo --no-sync       # only edit the manifest
skl add owner/repo --agent codex --only-profile work --os macos
skl configure review --agent codex --only-profile work
skl configure review notes --default-agents --all-os --all-profiles --no-sync

skl remove notes             # remove from the manifest and unlink

skl info review              # provenance, selection rules, content and link ownership
skl info pstack              # a plugin: pinned commit, folders, and installs
skl doctor                   # diagnose installation problems without changing files
skl verify                   # check pinned cached content offline using existing Git cache
skl verify review --json     # verify selected content with versioned JSON output
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
sources to edit them. A skill selected by a declared source's list or cached
wildcard membership gets a `source` override entry instead. It syncs
afterward unless `--no-sync` is supplied. Use
`--default-agents`, `--all-os`, or `--all-profiles` to remove an override and
restore the corresponding default; each reset conflicts with its setting flag.
Batch edits validate the whole resulting manifest before writing, preserve
comments and unspecified fields, and fail without changing the manifest if any
name is unknown or settings are invalid. Symlinked manifests remain symlinked.
`add` leaves existing entries unchanged, even when settings flags are supplied;
use `configure` to change those entries. `configure` and `remove` also work on
members of app sources such as Xcode.

`--profile <name>` works with every command and can be repeated.

`sync` and `list` exit non-zero when something needs attention, such as a conflict, a
skill that failed to fetch, or a missing link.

`skl info <name>` inspects a configured skill or an existing entry in configured
agent directories. It shows manifest source and ref, full pinned commit and
subdirectory (including stale lock entries), local paths or cached app provenance,
profile/OS selection reasons, and actual versus recorded link targets. A link is
owned only when its target still matches the state file. Old recorded destinations
are shown too, even after the configured agents change.

`info` is read-only: it takes no run lock, fetches nothing, runs no app export,
and creates no files. Exit 0 means the skill was found and inspected, even if it is
excluded, missing content, unlocked, or has conflicts. Unknown names, invalid
arguments, and unreadable or malformed metadata exit non-zero. Names from
wildcard sources become known after a successful sync; explicitly selected names can be
inspected before export. Unmanaged entries can be inspected even without a manifest. A plugin name shows
the plugin instead: its pinned commit, each agent's folder and store copy, and what
skl recorded installing.

`skl list` ends with a "Not managed by skl" section: every entry in each agent's
skills directory that `skl` doesn't own, such as skills an app installed or leftovers
from another tool, so you can see everything an agent will load.

## Shell completions

Generate completions from the installed CLI for Bash, Zsh, or Fish (Elvish and
PowerShell are also supported). Generation writes only the script to stdout and
requires no HOME, manifest, or installed skills. It completes commands, flags,
and predefined argument choices; skill names and profiles are not queried.

For Bash, load completions in the current shell:

```sh
source <(skl completions bash)
```

For Zsh, save the script in a directory on `fpath` before initializing completion:

```sh
mkdir -p ~/.zsh/completions
skl completions zsh > ~/.zsh/completions/_skl
```

Add these lines to `~/.zshrc` (before any existing `compinit` call):

```sh
fpath=(~/.zsh/completions $fpath)
autoload -Uz compinit
compinit
```

For Fish, install its automatically loaded completion file:

```sh
mkdir -p ~/.config/fish/completions
skl completions fish > ~/.config/fish/completions/skl.fish
```

Regenerate saved completion files after upgrading skl to pick up CLI changes.
