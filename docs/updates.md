---
title: Reviewing updates
---

[Documentation](index.md) · [GitHub](https://github.com/nanoxd/skl)

# Reviewing updates

Preview changes before updating pinned skills.

[Reviewing updates](#reviewing-updates) · [Saved update plans](#saved-update-plans)

## Reviewing updates

A skill is a prompt your agent acts on, often with access to your shell and files.
Updating a third-party skill changes what your agent will do, which makes it a
supply-chain change in the same way as bumping a dependency. Read it before you take
it.

Whenever a skill's commit changes, through `skl update` or a manifest edit, `skl`
prints a diffstat of that skill's directory between the old and new commit, or says
its files didn't change. A skill whose `source` changed only notes where it moved
from. Add `--patch` to see the full diff. To review without changing anything:

```sh
skl update --dry-run --patch
```

Then run `skl update` to apply it, or `skl update <name>` to take only the skills you
reviewed. The new commits land in `skills.lock`, so the change shows up in your
dotfiles history too.

## Saved update plans

Save exact Git commits and full patches for review, then install those same commits:

```sh
skl update --plan ./skills-update.toml
# Or select skills: skl update my-skill --plan ./skills-update.toml
less ./skills-update.toml
skl update --apply-plan ./skills-update.toml --dry-run
skl update --apply-plan ./skills-update.toml
```

Planning uses temporary repository caches and writes only the requested plan file.
The destination must be a new file in an existing directory outside skl's config,
data, agent skill directories and local skill directories. Existing files and
symlink aliases are never overwritten. `--plan` conflicts with `--dry-run`;
`--apply-plan` accepts `--dry-run` but cannot be combined with skill names or
`--plan`.

Plans contain the manifest and lock baseline, effective profiles, OS, machine
paths, resolved destinations, exact target commits, and full Git patches. Applying
rejects changed context instead of resolving newer branch tips. Selected updates
still reconcile the entire manifest, as ordinary `update` does: missing or stale
lock entries are included in the plan, and local skills and links are reconciled.
Local skill content is not snapshotted. Xcode manifests are not supported by saved
plans; use ordinary `update` for those manifests.

A failed planning run writes no plan. Apply uses the normal ownership checks and
keeps previous installs for skills that fail; successful skills may still update.
After a partial apply, generate a new plan against the new lock baseline. Treat
plans as trusted review artifacts: their checksum catches accidental edits but is
not a signature or authentication mechanism. Keep a plan private if its embedded
manifest contains private repository URLs or other sensitive configuration.
