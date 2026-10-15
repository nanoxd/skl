---
title: Diagnostics and scripting
---

[Documentation](index.md) · [GitHub](https://github.com/nanoxd/skl)

# Diagnostics and scripting

Use `skl info NAME` to inspect one skill and `skl doctor` to check your installation.

[Diagnostics](#diagnostics) · [JSON inspection](#json-inspection)

## Diagnostics

`skl doctor` checks the active profiles and OS for missing skills, invalid cached
`SKILL.md` files, missing or conflicting links, stale ownership records, and
required tools on PATH. It prints remedies and exits with status 1 when problems
are found (0 when healthy). It never changes files, takes a run lock, invokes
external tools, or contacts the network. It checks cached Xcode exports without
exporting new skills. Matching user-created links remain healthy and unowned. Local `SKILL.md`
symlinks are accepted; cached Git/Xcode skill files must be regular files. Diagnostics describe a point-in-time view; rerun after
a concurrent sync finishes.

## JSON inspection

Use `skl list --json`, `skl info NAME --json`, or `skl doctor --json` for scripts.
These commands stay offline and read-only. Stdout contains exactly one JSON
object, including when configuration, profile resolution, or filesystem reads
fail. Runtime errors do not also print human messages to stderr. Invalid command
syntax is handled by clap before execution and still uses stderr.

Every document has `schema_version: 1`, `command`, `status`, `data`, and
`diagnostics`. Status is `ok`, `unhealthy`, or `error`; `error` has `data: null`.
Each diagnostic has a stable `code`, `severity` (currently `error`), nullable
`skill` and `path`, a human-readable `message`, and a `remedy`. Scripts should
match codes rather than message text. File paths are absolute, including symlink
targets resolved relative to their link's parent; paths are not canonicalized.
Paths that cannot be represented as UTF-8 produce a structured runtime error.

- `info` data describes one skill: `name`, `manifest`, `origin` (`git`, `local`,
  `Xcode`, or `unmanaged`), optional `source`, `git_ref`, `subdir`, `locked`, and
  `xcode` provenance, `lock_matches`, `content_path`, `content_available`,
  `profiles`, `active_profiles`, `profile_source`, `os`, `enabled`, `exclusions`,
  `agents`, and `links`. Each link has `path`, optional `target` and
  `recorded_target`, `owned`, and `status`: `missing`, `unmanaged`, `broken`,
  `different_target`, `installed`, or `local_source`.
- `list` data has `skills` (the same reports, sorted by name), `unmanaged`
  (entries sorted by path, each with `agent`, `name`, `path`, `kind`, and nullable
  `target`), `active_profiles`, and `profile_source`. Unmanaged entries can also
  appear as conflicting links in a configured skill report. Hidden entries are
  omitted, as in human output.
- `doctor` data has `active_profiles` and `profile_source`. Its diagnostics use
  `stale-lock`, `missing-tool`, `xcode-not-locked`, `not-locked`, `invalid-cache`,
  `missing-local-skill`, `stale-state`, `missing-link`, `unreadable-link`,
  `outdated-link`, `broken-link`, or `link-conflict`.

Runtime failures use `runtime-error`. List health diagnostics use `not-linked`
(or `xcode-not-locked` for undiscovered wildcard imports). Exit codes match human
output: `info` succeeds when inspection completes, even if content or links are
missing; `doctor` fails when it finds any diagnostic; `list` fails for missing
active links or undiscovered active Xcode wildcard imports. List retains its
existing link-presence criteria; use doctor to check ownership and cache health.
Excluded skills remain in list data but do not cause missing-link failures.
