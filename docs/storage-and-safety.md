---
title: Storage and safety
---

[Documentation](index.md) · [GitHub](https://github.com/nanoxd/skl)

# Storage and safety

Understand link ownership, backups, and where skl keeps its files.

[Safety](#safety) · [Storage](#storage) · [Environment](#environment)

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
