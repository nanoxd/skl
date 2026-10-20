# skl

<img src="docs/assets/skl-mascot.png" alt="A playful skull mascot waving, carrying a backpack of skill cards" width="180">

File-based skill management for coding agents. Declare your skills in
`skills.toml`, commit it with `skills.lock`, and run `skl sync` to install the
pinned Git versions and link them into Claude Code, Codex, and other agents.
Keep the files in your dotfiles for your machines or in a project for your team.

[App-provided skills](docs/app-skills.md) can refresh from installed CLIs on each
sync; optional bundled paths skip machines without the app.

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

Create `~/.config/skl/skills.toml` in your editor (create the directory first if
needed):

```toml
agents = ["claude-code", "codex"]

[skills]
skill-creator = { source = "anthropics/skills", subdir = "skills/skill-creator" }
```

Then apply it:

```sh
skl sync
skl list
```

The manifest is the source of truth: edit it and run `skl sync` whenever you
change your selection. `skl add`, `skl configure`, and `skl remove` are shortcuts
that edit the same file and sync for you. See the annotated
[example manifest](examples/skills.toml) for more options.

To take several skills from one repository, declare it once as a source and list
the skills by name, or use `skills = "*"` for all of them:

```toml
[sources.anthropics]
source = "anthropics/skills"
skills = ["frontend-design", "skill-creator"]
```

Keep `~/.config/skl/skills.toml` and the generated `skills.lock` in your dotfiles.
On another machine, restore those files and run `skl sync --locked` to install
the pinned Git versions without changing the lockfile.

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
- [Finding and adding skills](docs/adding-skills.md) — search, repositories, local skills, and adopting installed skills
- [Configuration](docs/configuration.md) — manifest, declared sources, and lockfile
- [Agents and profiles](docs/agents-and-profiles.md) — destinations and machine selection
- [Project skills](docs/projects.md) — share skills with a team
- [All documentation](docs/index.md) — commands, updates, troubleshooting, Xcode, and CI

[Changelog](CHANGELOG.md) · [Contributing and releases](docs/maintaining.md) · [MIT license](LICENSE)
