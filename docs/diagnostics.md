---
title: Diagnostics and scripting
---

[Documentation](index.md) · [GitHub](https://github.com/nanoxd/skl)

# Diagnostics and scripting

Use `skl info NAME` to inspect one skill and `skl doctor` to check your installation.

[Diagnostics](#diagnostics) · [Content verification](#content-verification) · [JSON inspection](#json-inspection)

## Diagnostics

`skl doctor` checks the active profiles and OS for missing skills, invalid cached
`SKILL.md` files, missing or conflicting links, stale ownership records, and
required tools on PATH. It prints remedies and exits with status 1 when problems
are found (0 when healthy). It never changes files, takes a run lock, invokes
external tools, or contacts the network. It checks cached app exports, including
Xcode, without exporting new skills. Matching user-created links remain healthy and unowned. Local `SKILL.md`
symlinks are accepted; cached Git and app skill files must be regular files. Diagnostics describe a point-in-time view; rerun after
a concurrent sync finishes.

## Content verification

Run `skl verify` to compare cached content against pinned baselines, or select
skills with `skl verify review testing`. Like other commands it respects
`--profile` and `--project`. Disabled skills and local sources are explicitly
reported as `skipped`: local paths have no pinned content baseline.

For Git skills, verification uses only the existing bare repository cache and
exports the exact locked commit and subdirectory with `git archive` into a
removed-on-exit temporary directory. It compares every exported path against the
stored skill, including file contents, executable bits, types, symlink targets,
and added/deleted files and directories. Archive attributes such as
`export-ignore` apply just as they do during installation. Traversal compares
symlinks without following them; redirected skill roots or store ancestors are
rejected. Git and tar must be available. Verification never initializes a
repository or fetches, including Git's implicit partial-clone lazy fetching.
Missing cached commits or repositories produce `unavailable`, never `verified`.

Cached app skills, including Xcode's, are checked against their lockfile digest
without running the app or any external tools. The digest includes contents, paths, types, and
permissions. `verify` makes no repairs, takes no run lock, and changes no
manifest, lock, state, store, or agent links. Exit status is 1 for drift or
unavailable active baselines, including undiscovered wildcard sources, and 0
otherwise. Verification checks stored content; use `doctor` to inspect agent
links and ownership. These checks are a point-in-time observation; rerun after
concurrent updates. A verified result establishes agreement with the local lock
and cached Git objects, not the trustworthiness of third-party skill code.

## JSON inspection

Use `skl list --json`, `skl info NAME --json`, `skl doctor --json`, or
`skl verify [NAME ...] --json` for scripts.
These commands stay offline and read-only. Stdout contains exactly one JSON
object, including when configuration, profile resolution, or filesystem reads
fail. Runtime errors do not also print human messages to stderr. Invalid command
syntax is handled by clap before execution and still uses stderr.

Every document has `schema_version: 2`, `command`, `status`, `data`, and
`diagnostics`. Status is `ok`, `unhealthy`, or `error`; `error` has `data: null`.
Each diagnostic has a stable `code`, `severity` (currently `error`), nullable
`skill` and `path`, a human-readable `message`, and a `remedy`. Scripts should
match codes rather than message text. File paths are absolute, including symlink
targets resolved relative to their link's parent; paths are not canonicalized.
Paths that cannot be represented as UTF-8 produce a structured runtime error.

- `info` data describes one skill: `name`, `manifest`, `origin` (`git`, `local`,
  `command`, `app`, or `unmanaged`), optional `source`, `git_ref`, `subdir`,
  `locked`, and `command` provenance, `lock_matches`, `content_path`,
  `content_available`, `profiles`, `active_profiles`, `profile_source`, `os`,
  `enabled`, `exclusions`, `agents`, and `links`. For an app member, `source` is
  the alias of its declared source, and `command` has `producer`, `source`, and
  `snapshot`. Each link has `path`, optional `target` and
  `recorded_target`, `owned`, and `status`: `missing`, `unmanaged`, `broken`,
  `different_target`, `installed`, or `local_source`.
- `list` data has `skills` (the same reports, sorted by name), `unmanaged`
  (entries sorted by path, each with `agent`, `name`, `path`, `kind`, and nullable
  `target`), `active_profiles`, and `profile_source`. Unmanaged entries can also
  appear as conflicting links in a configured skill report. Hidden entries are
  omitted, as in human output.
- `doctor` data has `active_profiles` and `profile_source`. Its diagnostics use
  `stale-lock`, `missing-tool`, `source-not-locked`, `not-locked`, `invalid-cache`,
  `missing-local-skill`, `stale-state`, `missing-link`, `unreadable-link`,
  `outdated-link`, `broken-link`, or `link-conflict`.

- `verify` data has `active_profiles`, `profile_source`, and `skills`, sorted by
  name. Each skill has `name`, `source_kind` (`git`, `command`, `app`, or `local`),
  `status` (`verified`, `drift`, `unavailable`, or `skipped`), nullable absolute
  cache `path`, `message`, and `changes`. Changes have a path relative to the
  skill directory and `kind` (`added`, `deleted`, `changed`, or `digest-changed`).
  App digest changes use path `.`. Diagnostics use `content-drift` and
  `baseline-unavailable`; either causes `status: "unhealthy"` and exit status 1.

Runtime failures use `runtime-error`. List health diagnostics use `not-linked`
(or `source-not-locked` for undiscovered wildcard sources). Exit codes match human
output: `info` succeeds when inspection completes, even if content or links are
missing; `doctor` fails when it finds any diagnostic; `list` fails for missing
active links or undiscovered active wildcard sources. List retains its
existing link-presence criteria; use doctor to check ownership and cache health.
Excluded skills remain in list data but do not cause missing-link failures.
