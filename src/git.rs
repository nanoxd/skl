use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail};

/// Shells out to git so private repos use the same SSH keys and credential helpers
/// as everything else on the machine.
pub struct Git {
    repos: PathBuf,
}

/// A directory in a repo that holds a SKILL.md.
pub struct Candidate {
    pub subdir: String,
    /// The `name` from SKILL.md's frontmatter, falling back to the directory name.
    pub name: String,
    pub dir_name: String,
}

impl Git {
    pub fn new(repos: PathBuf) -> Self {
        Self { repos }
    }

    pub fn repo_dir(&self, url: &str) -> PathBuf {
        self.repos.join(format!("{}.git", cache_key(url)))
    }

    /// Resolves a branch, tag or HEAD on the remote to a commit. Full SHAs are returned
    /// as is, since a remote can't be asked for them by name.
    pub fn resolve(&self, url: &str, git_ref: Option<&str>) -> Result<String> {
        if let Some(sha) = git_ref.filter(|r| is_full_sha(r)) {
            return Ok(sha.to_ascii_lowercase());
        }
        let pattern = git_ref.unwrap_or("HEAD");
        let out = run(git().args(["ls-remote", url, pattern]))
            .with_context(|| format!("looking up {pattern} in {url}"))?;
        let refs: Vec<(&str, &str)> = out
            .lines()
            .filter_map(|line| line.split_once('\t'))
            .collect();
        let exact = |name: String| refs.iter().find(|(_, r)| *r == name).map(|(sha, _)| *sha);
        let found = match git_ref {
            None => exact("HEAD".into()),
            Some(r) => exact(format!("refs/tags/{r}^{{}}"))
                .or_else(|| exact(format!("refs/tags/{r}")))
                .or_else(|| exact(format!("refs/heads/{r}")))
                .or_else(|| exact(r.into())),
        };
        match found {
            Some(sha) => Ok(sha.into()),
            None if git_ref
                .is_some_and(|r| r.len() >= 7 && r.chars().all(|c| c.is_ascii_hexdigit())) =>
            {
                bail!("`{pattern}` looks like a short commit SHA: use the full 40 characters")
            }
            None => bail!("{url} has no branch or tag named `{pattern}`"),
        }
    }

    /// Makes sure the commit is in the local cache, fetching only that commit when the
    /// remote allows it.
    pub fn ensure_commit(&self, url: &str, commit: &str) -> Result<PathBuf> {
        let repo = self.repo_dir(url);
        if !repo.join("HEAD").exists() {
            fs::create_dir_all(&self.repos)?;
            run(git().args(["init", "--bare", "-q"]).arg(&repo))?;
        }
        if has_commit(&repo, commit) {
            return Ok(repo);
        }
        let shallow = run(git().arg("-C").arg(&repo).args([
            "fetch",
            "-q",
            "--no-tags",
            "--depth",
            "1",
            url,
            commit,
        ]));
        if shallow.is_err() || !has_commit(&repo, commit) {
            let mut full = git();
            full.arg("-C").arg(&repo).args(["fetch", "-q", "--no-tags"]);
            if run(git()
                .arg("-C")
                .arg(&repo)
                .args(["rev-parse", "--is-shallow-repository"]))?
            .trim()
                == "true"
            {
                full.arg("--unshallow");
            }
            full.args([
                url,
                "+refs/heads/*:refs/heads/*",
                "+refs/tags/*:refs/tags/*",
            ]);
            run(&mut full).with_context(|| format!("fetching {url}"))?;
        }
        if !has_commit(&repo, commit) {
            bail!("{url} has no commit {commit}");
        }
        Ok(repo)
    }

    pub fn candidates(&self, repo: &Path, commit: &str) -> Result<Vec<Candidate>> {
        let files = run(git()
            .arg("-C")
            .arg(repo)
            .args(["ls-tree", "-r", "-z", commit]))?;
        let mut found = Vec::new();
        for entry in files.split('\0') {
            // `<mode> <type> <sha>\t<path>`. Only regular files count, so a symlinked
            // SKILL.md that might point anywhere is never taken for a skill.
            let Some((meta, file)) = entry.split_once('\t') else {
                continue;
            };
            if !meta.starts_with("100") {
                continue;
            }
            let subdir = if file == "SKILL.md" {
                ""
            } else if let Some(dir) = file.strip_suffix("/SKILL.md") {
                dir
            } else {
                continue;
            };
            let text = run(git().arg("-C").arg(repo).args([
                "cat-file",
                "-p",
                &format!("{commit}:{file}"),
            ]))?;
            let dir_name = match subdir.rsplit('/').next() {
                Some(name) if !name.is_empty() => name.to_string(),
                _ => repo_name(repo),
            };
            let name = frontmatter_name(&text).unwrap_or_else(|| dir_name.clone());
            found.push(Candidate {
                subdir: subdir.into(),
                name,
                dir_name,
            });
        }
        // Repos often mirror a skill into tool-specific hidden folders. Prefer the
        // copies outside hidden directories when there are any.
        let hidden = |c: &Candidate| c.subdir.split('/').any(|part| part.starts_with('.'));
        if found.iter().any(|c| !hidden(c)) {
            found.retain(|c| !hidden(c));
        }
        Ok(found)
    }

    /// Compares two trees, such as `<commit>:<dir>`, as a diffstat, plus the full
    /// diff when `patch` is set. Empty when nothing changed.
    pub fn diff(&self, repo: &Path, old: &str, new: &str, patch: bool) -> Result<String> {
        let mut cmd = git();
        cmd.arg("-C").arg(repo).args([
            "diff",
            "--no-color",
            "--no-ext-diff",
            "--stat=100",
            "--summary",
        ]);
        if patch {
            cmd.arg("--patch");
        }
        run(cmd.args([old, new]))
    }

    /// Writes one directory of `commit` into `dest`. The caller extracts to a temporary
    /// directory and renames it into place.
    pub fn export(&self, repo: &Path, commit: &str, subdir: &str, dest: &Path) -> Result<()> {
        let tree = if subdir.is_empty() {
            commit.to_string()
        } else {
            format!("{commit}:{subdir}")
        };
        fs::create_dir_all(dest)?;
        let mut archive = git()
            .arg("-C")
            .arg(repo)
            .args(["archive", "--format=tar", &tree])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .context("running git archive")?;
        let tar = Command::new("tar")
            .arg("-xf")
            .arg("-")
            .arg("-C")
            .arg(dest)
            .stdin(archive.stdout.take().expect("piped stdout"))
            .output()
            .context("running tar")?;
        let archived = archive.wait_with_output()?;
        if !archived.status.success() {
            bail!(
                "git archive {commit} failed: {}",
                String::from_utf8_lossy(&archived.stderr).trim()
            );
        }
        if !tar.status.success() {
            bail!(
                "extracting {commit} failed: {}",
                String::from_utf8_lossy(&tar.stderr).trim()
            );
        }
        Ok(())
    }
}

pub fn frontmatter_name(text: &str) -> Option<String> {
    let mut lines = text.lines();
    if lines.next()?.trim() != "---" {
        return None;
    }
    for line in lines {
        let line = line.trim_end();
        if line == "---" {
            break;
        }
        if let Some(value) = line.strip_prefix("name:") {
            let value = value.trim().trim_matches(|c| c == '"' || c == '\'').trim();
            return (!value.is_empty()).then(|| value.to_string());
        }
    }
    None
}

/// A filesystem-safe name for a repo, shared by the git cache and the store.
pub fn cache_key(url: &str) -> String {
    let trimmed = url
        .split_once("://")
        .map_or(url, |(_, rest)| rest)
        .trim_start_matches("git@")
        .trim_end_matches('/')
        .trim_end_matches(".git");
    let key: String = trimmed
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || "-_.".contains(c) {
                c
            } else {
                '_'
            }
        })
        .collect();
    key.trim_matches('_').to_string()
}

fn repo_name(repo: &Path) -> String {
    let file = repo
        .file_name()
        .map(|f| f.to_string_lossy().into_owned())
        .unwrap_or_default();
    let key = file.trim_end_matches(".git");
    key.rsplit('_').next().unwrap_or(key).to_string()
}

fn is_full_sha(value: &str) -> bool {
    value.len() == 40 && value.chars().all(|c| c.is_ascii_hexdigit())
}

fn has_commit(repo: &Path, commit: &str) -> bool {
    git()
        .arg("-C")
        .arg(repo)
        .args(["cat-file", "-e", &format!("{commit}^{{commit}}")])
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

fn git() -> Command {
    let mut cmd = Command::new("git");
    // Fail instead of hanging on a credential prompt nobody can see.
    cmd.env("GIT_TERMINAL_PROMPT", "0");
    cmd
}

fn run(cmd: &mut Command) -> Result<String> {
    let out = cmd.stdin(Stdio::null()).output().context("running git")?;
    if !out.status.success() {
        bail!("{}", String::from_utf8_lossy(&out.stderr).trim());
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}
