---
title: Getting started
---

[Documentation](index.md) · [GitHub](https://github.com/nanoxd/skl)

# Getting started

Describe your skills in a file, apply it with skl, and share it through dotfiles.

[Install](#install) · [Quick start](#quick-start) · [Dotfiles integration](#dotfiles-integration)

## Install

With Homebrew (macOS or Linux):

```sh
brew tap nanoxd/skl https://github.com/nanoxd/skl
brew install nanoxd/skl/skl
```

Or build from source with Cargo:

```sh
cargo install --git https://github.com/nanoxd/skl
```

Prebuilt binaries for macOS (Apple Silicon and Intel) and Linux (x86_64 and ARM64, glibc) are attached to each
[release](https://github.com/nanoxd/skl/releases). `skl` needs `git` and `tar` on
your `PATH`.

The formula installs a checksum-verified release binary for ARM64 or x86_64;
no Rust toolchain is needed. The explicit tap URL is required because this
repository is named `skl`, rather than `homebrew-skl`.

To upgrade:

```sh
brew update
brew upgrade nanoxd/skl/skl
```

## Quick start

Create `~/.config/skl/skills.toml` in your editor. Create the directory first if
it doesn't exist. This manifest describes the skills you want and which agents
should receive them:

```toml
agents = ["claude-code", "codex"]

[skills]
skill-creator = { source = "anthropics/skills", subdir = "skills/skill-creator" }
```

For more options, see the annotated
[example manifest](https://github.com/nanoxd/skl/blob/main/examples/skills.toml)
and [configuration reference](configuration.md).

Apply the manifest:

```sh
skl sync --dry-run  # Preview the changes
skl sync           # Fetch, lock, and link the skills
```

`sync` generates `~/.config/skl/skills.lock` with the Git commit it installed and
links the skill into `~/.claude/skills` and `~/.agents/skills`. Commit the manifest
and lockfile to your dotfiles. You edit the manifest; `skl` maintains the lockfile.

To change your setup, edit `skills.toml` and run `skl sync` again. Adding an entry
installs a skill; removing an entry removes the links `skl` owns for that skill.
Existing Git skills keep their pinned versions until you run `skl update` or
change their source, ref, or subdirectory.

The CLI offers shortcuts for editing the same manifest and syncing it:

```sh
skl add anthropics/skills --skill skill-creator
skl configure skill-creator --agent codex
skl remove skill-creator
```

On every other machine, restore those two files from your dotfiles and run:

```sh
skl sync --locked
```

`--locked` installs the pinned Git versions and fails if the manifest would
require a lockfile change. Local `path` skills link directly to their source
directories, so restore those directories too; their contents aren't pinned.

## Dotfiles integration

Commit `~/.config/skl` with your dotfiles, or link `skills.toml` and `skills.lock`
into it from your dotfiles repo. To bring a machine up to date whenever you pull, add
a post-merge hook to the dotfiles repo:

```sh
#!/bin/sh
# .git/hooks/post-merge
command -v skl >/dev/null && skl sync
```

In a bootstrap script, install `skl` after linking your dotfiles, set the machine's
profile, and sync:

```sh
cargo install --git https://github.com/nanoxd/skl
skl profile set personal
skl sync
```
