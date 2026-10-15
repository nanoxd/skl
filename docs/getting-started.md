---
title: Getting started
---

[Documentation](index.md) · [GitHub](https://github.com/nanoxd/skl)

# Getting started

Install skl, add your first skills, and share them through dotfiles.

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

On the first machine, add skills:

```sh
skl add example/notes
skl add example/agent-skills --skill review
skl add ~/dev/skills/my-skill
```

Each `add` edits `~/.config/skl/skills.toml`, resolves the skill to a commit in
`skills.lock`, and links it into `~/.claude/skills` and `~/.agents/skills`. Commit
both files to your dotfiles.

On every other machine, pull your dotfiles and run:

```sh
skl sync
```

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
