---
title: Project skills
---

[Documentation](index.md) · [GitHub](https://github.com/nanoxd/skl)

# Project skills

Share a pinned set of skills with everyone working in a repository.

## Project scope

Use `--project PATH` to share skills with a team through a repository:

```sh
skl --project . add example/agent-skills --skill review
skl --project . add ./skills/my-skill
skl --project . sync --locked
```

The project directory must already exist. Commit `skills.toml` and `skills.lock`
at its root. Local sources inside the project are written as relative paths and
resolve against that root on every machine. The default agents install into
`<project>/.claude/skills` and `<project>/.agents/skills`. Other named agents use
their project directories; `skl --project . agents list` shows the full mapping.
Project-only harnesses Eve and PromptScript are also supported. Custom agent
paths starting with `./` resolve against the project root; explicit `~/` and
absolute paths still work and intentionally select those locations.

Scope is always explicit: running `skl sync` inside the repository still uses
the global manifest. `--project` works before or after the subcommand. It ignores
`SKL_CONFIG_DIR` for manifest lookup, and does not search parent directories.
Profiles, ownership state, run locks, backups, git caches, and the content store
are isolated under `<data>/projects/<SHA-256 of canonical project path>`;
`<data>` follows the usual `SKL_DATA_DIR` / XDG settings. Global installs and
separate checkouts never share ownership or garbage collection. Runtime data
stays outside the repository.

Ignore the generated agent skill links in git; teammates recreate them with
`skl --project . sync --locked`. A fresh checkout needs only the manifest, lock,
and any local skill sources. Moving an existing working tree with generated
links keeps those links pointing at the old cache. They are treated as unowned
conflicts at the new root: use `skl --project . sync --locked --force` to back
them up and rebuild them. Old project caches are retained and can be removed
manually once no longer needed.
