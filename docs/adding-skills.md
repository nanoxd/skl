---
title: Finding and adding skills
---

[Documentation](index.md) · [GitHub](https://github.com/nanoxd/skl)

# Finding and adding skills

Search the catalog or inspect a source before choosing what to install.

[Searching the catalog](#searching-the-catalog) · [Browsing repositories](#browsing-repositories) · [Source shortcuts](#source-shortcuts) · [Local collections](#local-collections) · [Interactive installation](#interactive-installation)

## Searching the catalog

```sh
skl search swift testing
skl search review --owner anthropics --limit 10
skl search rust --json
```

`search` queries the public [skills.sh catalog](https://skills.sh/) and prints
repository, skill name, install count, and a `skl add` command for each result.
Results are sorted by install count. `--limit` defaults to 20 and accepts 1–100;
`--owner` restricts results to a GitHub owner. Search does not install anything,
read local configuration, require `HOME`, or create files.

Your query, limit, and optional owner are sent to
`https://skills.sh/api/search`. `SKL_SEARCH_URL` can override the complete endpoint
URL for another compatible service or local testing. Requests have a 15-second
timeout and responses are limited to 1 MiB. HTTP failures, malformed responses,
and invalid repository/skill identities produce errors rather than empty results.
Remote identities must be valid before they are displayed or used in suggested
commands.

`--json` uses the same versioned envelope as inspection commands, with
`command: "search"`. Successful `data` contains `query`, nullable `owner`, `limit`,
and `skills`; each skill has `name`, `source`, `installs`, and `install_command`.
Empty results are successful with an empty `skills` array. Runtime errors use
`status: "error"`, `data: null`, and `runtime-error` diagnostics, with no additional
stderr output. Invalid CLI syntax still uses clap's usual stderr errors.

## Browsing repositories

Run `skl discover owner/repo` (or pass a git URL) to see the resolved commit,
all available skill names, their subdirectories, and descriptions before installing.
Use `--ref` to inspect a branch, tag, or full commit SHA:

```sh
skl discover anthropics/skills
skl discover owner/repo --ref v1.0
```

Discovery needs no manifest and uses a temporary repository cache that is removed
when the command finishes. It does not change configuration, installed skills,
links, or the run lock. A root skill has subdirectory `.`. Duplicate names remain
separate rows so their locations are visible. As with `add`, symlinked `SKILL.md`
files are excluded, and hidden-directory copies are omitted only when a visible skill has the same name.
Descriptions support common plain, quoted, and multiline block frontmatter scalars;
missing or unsupported descriptions are blank. Empty repositories with a commit
but no skills produce a successful “No skills found” result.

## Source shortcuts

`add` and `discover` accept `owner/repo@skill` and GitHub or GitLab tree URLs:

```sh
skl add vercel-labs/agent-skills@web-design-guidelines
skl add https://github.com/owner/repo/tree/main/skills/review
skl discover https://gitlab.com/group/project/-/tree/main/skills/review
skl add owner/repo --ref main --subdir skills/review
```

Tree paths and `add --subdir` select the exact directory containing `SKILL.md`,
including hidden directories; use `--subdir .` for the repository root. The
manifest stores the repository, ref, and directory separately, so subsequent
syncs and updates use the same selection. `@skill` selects by skill or directory
name and cannot be combined with a different `--skill` or with `--all`.

A tree URL normally uses the first segment after `tree/` as its ref. If an
advertised branch or tag containing slashes also matches the URL, skl asks for
an explicit `--ref` rather than guessing. For example, append
`--ref feature/topic` to a URL ending in `tree/feature/topic/skills/review`.
Explicit refs must match the URL; an explicit directory must agree with its
path. To override either, use the repository URL with `--ref` and `--subdir`.
Tree URLs with percent escapes, fragments, or query strings are rejected with
instructions to use those flags. Ordinary git URLs, SSH credentials and private
repository authentication continue to pass through to git unchanged.

## Local collections

Import several skills from a local directory without copying their contents:

```sh
skl add ./skills --all
skl add ./skills --skill review --skill testing
```

Collections are searched recursively, including hidden directories; `.git` and
symlinked directories or `SKILL.md` files are skipped. Visible skills take
precedence over hidden copies with the same name. Other duplicate names fail
before the manifest changes; add their individual directories with distinct
`--skill` names to disambiguate. Selections match frontmatter names first, then
directory names. Without selection flags, a collection containing several skills
asks you to choose. `--ref` is only for Git sources.

Each selected directory becomes an ordinary local `path` entry, so edits are
immediately visible to agents and no Git lock entries are created. Paths beneath
the config directory are saved relative to it for portable dotfiles; other paths
use `~/...` when possible. The collection itself is not tracked: rerun `add --all`
to include newly created skills. A directory with its own `SKILL.md` retains the
single-skill behavior, including `--skill NAME` to rename it; use `--all` to scan
its nested skills too. Existing direct local skills may still use a symlinked
`SKILL.md`; collection discovery only accepts regular files.

## Interactive installation

Use `skl add owner/repo --interactive` to choose skills and agent destinations
with numbered prompts. Enter comma-separated numbers or `all`; enter `q`, an
empty line, or end-of-input to cancel. Invalid selections stop the command.
Cancellation and invalid input do not write the manifest, links, or shared state.
Skill discovery uses disposable storage while you choose; skl acquires its write
lock and revalidates the selection before editing the manifest.

Explicit `--skill`, `@skill`, or `--all` skips the skill prompt. Explicit `--agent`
skips the agent prompt. Local directories use the same collection discovery as
`add --all`; selecting a nested skill keeps its frontmatter name and exact path,
even when the collection root also contains a skill.
Selected agents become per-skill overrides; existing manifest entries retain their
settings, as with ordinary `add`. Without `--interactive`, behavior remains
noninteractive. The flag requires both stdin and stderr to be terminals; use
explicit flags in scripts.

`skl agents detect` reports recognizable harness configuration directories without
changing files. The interactive agent list marks these hints, but still lets you
choose any known harness. A configuration directory does not prove an agent is
installed, and shared directories such as `~/.agents/skills` do not identify all
the harnesses that might read them. Detection never changes your default agents. With `--project`, detection examines
that project instead of your home directory, and selection includes project-only
harnesses such as Eve. Shared project skill directories do not identify individual
harnesses either.

## Adopting installed skills

`list` also shows skills skl does not manage. Choose an installed directory name
and its agent explicitly, then preview a durable local snapshot:

```sh
skl list
skl adopt review --from claude-code --dry-run
skl adopt review --from claude-code --no-sync
skl sync --dry-run
```

`adopt` copies the directory into `local/review` beside `skills.toml` and adds
`review = { path = "local/review" }`. Commit both the manifest and `local/` to
your dotfiles to carry the snapshot to another machine. This is a one-time copy;
future source edits and `skl update` do not update it. Remote provenance is never
inferred. Use `add SOURCE` instead when you already know the Git source to track,
or `add DIRECTORY` when you deliberately want a live local path.

Without `--no-sync`, adoption runs ordinary sync after registering the snapshot.
The original directory or user symlink is preserved and remains an unmanaged
conflict; other available destinations receive links. Sync can therefore report
failure even though the snapshot and manifest were saved. After reviewing
`skl sync --dry-run`, explicitly run `skl sync --force` if you want existing
installations backed up and replaced using skl's normal ownership rules. Adoption
never silently claims an existing link, even when it points into skl's store.

`--from` accepts a known agent or a custom absolute/`~/...` skills directory.
`--agent`, `--only-profile`, and `--os` set the snapshot's destinations and
selection restrictions just as with `add`; absent overrides use manifest defaults.
`--project PATH adopt ...` reads that project's agent directories and writes the
snapshot beside its project manifest. These commands never discover or import
other skills implicitly.

A top-level installed symlink is copied from its resolved directory, leaving the
link unchanged. The source must contain a regular `SKILL.md`; nested symlinks
(including cycles) and special files are rejected rather than retaining transient
references. An existing snapshot destination, an existing manifest name, or an
already owned installation is rejected before copying. Failed copying leaves no
published snapshot, and failed manifest publication removes the new snapshot.
`--dry-run` validates and previews without writing configuration, state, locks,
store content, or links.

Names supplied by cached repository collections or Xcode imports also count as
existing declarations. If wildcard membership has not been cached completely,
run `skl sync` first so adoption can check for collisions before copying.
