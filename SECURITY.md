# Security

`skl` installs skills, which are prompts that coding agents act on, often with access
to your shell and files. A bug that installs the wrong content, follows a link out of
the store, or deletes files skl didn't create is a security issue.

Please report vulnerabilities privately through
[GitHub's security advisories](https://github.com/nanoxd/skl/security/advisories/new)
rather than in a public issue. Include the version or commit, your manifest if it's
relevant, and steps to reproduce.

The content of a skill is the responsibility of whoever publishes it. Review
third-party updates before taking them with `skl update --dry-run --patch`.
