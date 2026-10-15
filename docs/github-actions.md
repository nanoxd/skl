---
title: GitHub Actions
---

[Documentation](index.md) · [GitHub](https://github.com/nanoxd/skl)

# GitHub Actions

Install a pinned release in CI.

## GitHub Actions

In GitHub Actions, install an exact release without a Rust toolchain:

```yaml
steps:
  - uses: actions/checkout@v7
  - uses: nanoxd/skl/.github/actions/setup-skl@main
    with:
      version: v0.1.0
  - run: skl sync --locked
```

Pin the action itself to a reviewed full commit SHA for reproducible workflows.
The required `version` is an exact release tag; `latest` is deliberately rejected.
The action downloads the matching macOS or glibc Linux binary for ARM64 or x86_64,
verifies its release SHA-256 sidecar, and adds it to `PATH`. It requires `git` and
`tar` for subsequent skill operations, and `curl` and `shasum` for installation;
GitHub-hosted macOS and Ubuntu runners include these tools. It does not sync any
skills or edit configuration. Provision your manifest and lockfile before running
`sync`, for example by checking out your dotfiles and setting `SKL_CONFIG_DIR`.

The checksum detects corrupt downloads; it is published alongside the binary,
not an independent signature. No GitHub token or repository write permission is
needed to install public releases.
