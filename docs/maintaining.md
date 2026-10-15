---
title: Maintaining skl
---

[Documentation](index.md) · [GitHub](https://github.com/nanoxd/skl)

# Maintaining skl

Development, releases, and documentation publishing.

## Development

From the repository root, run:

```sh
cargo build --all-features
cargo test --all-features
cargo fmt --check
cargo clippy --all-features --all-targets -- -D warnings
```

See [AGENTS.md](https://github.com/nanoxd/skl/blob/main/AGENTS.md) for architecture,
conventions, and safety invariants.

## Releasing

Keep release notes under `## [Unreleased]` in `CHANGELOG.md`. Push a `v*` tag
(for example, `v0.2.0`) to build and publish a release. The release workflow
moves those notes into `## [0.2.0] - YYYY-MM-DD` using the UTC release date,
leaves an empty `[Unreleased]` section, and uses the entry as GitHub release notes.
An existing entry for the exact version takes precedence. Missing or empty notes
fail the release instead of publishing blank notes.

After publishing, the workflow commits the changelog update to the repository's
default branch. It applies only the changelog change from the tagged snapshot;
the tag stays on its original commit. The Actions token must be allowed to push
to that branch. Conflicting changelog edits or a concurrent push fail the save
step without overwriting branch changes. Resolve any conflict and rerun the
workflow; reruns update the existing release and skip already saved entries.
Pull the automated commit before preparing the next release.

This automates changelog headings, not the package version in `Cargo.toml`.

## Maintaining the Homebrew formula

The tag release workflow updates all four release URLs and SHA-256 values in
[`Formula/skl.rb`](https://github.com/nanoxd/skl/blob/main/Formula/skl.rb) after uploading the binaries. It verifies each
archive against its checksum sidecar, then commits the formula to the default
branch. `brew update` picks up that commit. Prerelease/build-metadata tags and
older versions leave the stable formula unchanged.

The workflow needs permission to push to the default branch, just like its
changelog update. Protected-branch rules may require allowing the release bot.
Concurrent pushes fail safely without force-pushing; rerun the failed release
job to retry. Repeating an unchanged release does not create another formula
commit. To update manually, download all four archives and `.sha256` sidecars
into `dist`, then run
`cargo run --locked --features maintainer-tools --bin update_homebrew -- vX.Y.Z dist`.

Validate the updated formula with `brew style nanoxd/skl/skl`, then reinstall
and run `brew test nanoxd/skl/skl` using a tap checkout containing the update.
The formula test installs, syncs, and removes a local skill in a temporary home.
See Homebrew's [tap documentation](https://docs.brew.sh/How-to-Create-and-Maintain-a-Tap)
for working with a local tap checkout.

## Repository examples and tools

[`examples/skills.toml`](https://github.com/nanoxd/skl/blob/main/examples/skills.toml)
is an annotated starter manifest for users. Copy it to `~/.config/skl/skills.toml`
or a project's root and adapt it before syncing.

The `tools/` directory contains the maintainer binaries used by the
release workflow: `prepare_release` prepares changelog entries and release notes;
`update_homebrew` updates the formula from verified release artifacts. Run them
from the repository root with
`cargo run --locked --features maintainer-tools --bin NAME -- …`.
The `maintainer-tools` feature enables these binaries for release work; normal
builds and installs include only the `skl` CLI.

## Publishing the documentation

The Markdown files in `docs/` also form a GitHub Pages site. Edit them once;
relative links work in the repository and are converted to HTML links by
[jekyll-relative-links](https://github.com/benbalter/jekyll-relative-links).
The site uses GitHub's Primer theme and needs no custom build workflow.

After these files reach `main`, enable publishing in the repository's
**Settings → Pages**: select **Deploy from a branch**, choose **main** and
**/docs**, then save. See GitHub's
[publishing source instructions](https://docs.github.com/en/pages/getting-started-with-github-pages/configuring-a-publishing-source-for-your-github-pages-site).
The expected address is `https://nanoxd.github.io/skl/`; repository settings must
be enabled before it is live. For a fork or custom domain, update `url`,
`baseurl`, and `repository` in `docs/_config.yml`.

Keep the README focused on installation and first use. Add detailed guidance to
the relevant page and link new pages from [the documentation index](index.md).
Each page needs YAML front matter so Jekyll renders it.
