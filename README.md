# skl

<img src="docs/assets/skl-mascot.png" alt="A playful skull mascot waving, carrying a backpack of skill cards" width="180">

Keep agent skills in sync across coding agents and machines. `skl` tracks what to
install in a manifest, pins Git versions in a lockfile, and links skills into
Claude Code, Codex, and other agents. It leaves unmanaged files alone.

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

Or download a [prebuilt binary](https://github.com/nanoxd/skl/releases) for macOS
or Linux. Requires `git` and `tar`.

## Get started

```sh
skl add anthropics/skills --skill skill-creator
skl list
```

Keep `~/.config/skl/skills.toml` and `skills.lock` in your dotfiles. On another
machine, restore those files and run `skl sync` to install the same versions.

```sh
skl update --dry-run --patch  # Review available updates
skl update                  # Apply updates
skl remove skill-creator    # Remove a managed skill
```

For a team's repository, use `skl --project . add …` and commit the manifest and
lockfile at the project root. Teammates run `skl --project . sync --locked`.

Generate shell completions with `skl completions bash`, `skl completions zsh`, or
`skl completions fish`. See [setup instructions](docs/commands.md#shell-completions).

## Documentation

- [Getting started](docs/getting-started.md) — installation and dotfiles setup
- [Finding and adding skills](docs/adding-skills.md) — search, repositories, and local skills
- [Configuration](docs/configuration.md) — manifest, repository groups, and lockfile
- [Agents and profiles](docs/agents-and-profiles.md) — destinations and machine selection
- [Project skills](docs/projects.md) — share skills with a team
- [All documentation](docs/index.md) — commands, updates, troubleshooting, Xcode, and CI

[Changelog](CHANGELOG.md) · [Contributing and releases](docs/maintaining.md) · [MIT license](LICENSE)
