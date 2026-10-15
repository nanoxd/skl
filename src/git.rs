use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result, bail};

/// Shells out to git so private repos use the same SSH keys and credential helpers
/// as everything else on the machine.
pub struct Git {
    repos: PathBuf,
    fetch_locks: Mutex<BTreeMap<String, Arc<Mutex<()>>>>,
}

/// A directory in a repo that holds a SKILL.md.
pub struct Candidate {
    pub subdir: String,
    /// The `name` from SKILL.md's frontmatter, falling back to the directory name.
    pub name: String,
    pub dir_name: String,
    pub description: Option<String>,
}

impl Git {
    pub fn new(repos: PathBuf) -> Self {
        Self {
            repos,
            fetch_locks: Mutex::new(BTreeMap::new()),
        }
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

    /// A tree URL cannot distinguish slash-containing refs from directories.
    pub fn check_tree_ref(&self, url: &str, tail: &str) -> Result<()> {
        let refs = run(git().args(["ls-remote", "--heads", "--tags", url]))?;
        for line in refs.lines() {
            let Some((_, name)) = line.split_once('\t') else {
                continue;
            };
            let Some(name) = name
                .strip_prefix("refs/heads/")
                .or_else(|| name.strip_prefix("refs/tags/"))
            else {
                continue;
            };
            if name.contains('/')
                && (tail == name
                    || tail
                        .strip_prefix(name)
                        .is_some_and(|rest| rest.starts_with('/')))
            {
                bail!(
                    "tree URL may refer to `{name}`; specify --ref explicitly to separate the ref from the skill directory"
                );
            }
        }
        Ok(())
    }

    /// Makes sure the commit is in the local cache, fetching only that commit when the
    /// remote allows it.
    pub fn ensure_commit(&self, url: &str, commit: &str) -> Result<PathBuf> {
        // Different commits of one repository share a bare cache. Serialize its
        // initialization and fetches, while unrelated repositories stay parallel.
        let guard = self
            .fetch_locks
            .lock()
            .unwrap()
            .entry(cache_key(url))
            .or_default()
            .clone();
        let _guard = guard.lock().unwrap();
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
        self.candidates_in(repo, commit, None)
    }

    pub fn candidates_in(
        &self,
        repo: &Path,
        commit: &str,
        selected: Option<&str>,
    ) -> Result<Vec<Candidate>> {
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
            if selected
                .is_some_and(|directory| (if directory == "." { "" } else { directory }) != subdir)
            {
                continue;
            }
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
                description: frontmatter_description(&text),
            });
        }
        prefer_visible_skills(&mut found);
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

    /// Export a baseline using only an existing repository. Never initialize or fetch,
    /// including Git's implicit lazy fetching from partial clones.
    pub fn export_cached(&self, url: &str, commit: &str, subdir: &str, dest: &Path) -> Result<()> {
        if !is_full_sha(commit) {
            bail!("baseline requires a full locked commit SHA");
        }
        let repo = self.repo_dir(url);
        if !repo.join("HEAD").is_file() {
            bail!("cached repository is unavailable: {}", repo.display());
        }
        self.export_inner(&repo, commit, subdir, dest, true)
    }

    /// Writes one directory of `commit` into `dest`. The caller extracts to a temporary
    /// directory and renames it into place.
    pub fn export(&self, repo: &Path, commit: &str, subdir: &str, dest: &Path) -> Result<()> {
        self.export_inner(repo, commit, subdir, dest, false)
    }

    fn export_inner(
        &self,
        repo: &Path,
        commit: &str,
        subdir: &str,
        dest: &Path,
        offline: bool,
    ) -> Result<()> {
        let tree = if subdir.is_empty() {
            commit.to_string()
        } else {
            format!("{commit}:{subdir}")
        };
        fs::create_dir_all(dest)?;
        let mut command = git();
        if offline {
            command
                .env("GIT_NO_LAZY_FETCH", "1")
                .env("GIT_ALLOW_PROTOCOL", "")
                .env("GIT_OPTIONAL_LOCKS", "0")
                .args(["-c", "protocol.allow=never"]);
        }
        let mut archive = command
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

/// A display-only description. Handle common single-line and block scalars without
/// interpreting arbitrary YAML or changing the name used by `add`.
fn frontmatter_description(text: &str) -> Option<String> {
    let mut lines = text.lines();
    if lines.next()?.trim() != "---" {
        return None;
    }
    let header: Vec<_> = lines
        .by_ref()
        .take_while(|line| line.trim_end() != "---")
        .collect();
    // A frontmatter section must be closed; do not inspect Markdown body text.
    if !text.lines().skip(1).any(|line| line.trim_end() == "---") {
        return None;
    }
    let (index, value) = header.iter().enumerate().find_map(|(i, line)| {
        line.strip_prefix("description:")
            .map(|value| (i, value.trim()))
    })?;
    let description = if matches!(value, "|" | "|-" | "|+" | ">" | ">-" | ">+") {
        header[index + 1..]
            .iter()
            .take_while(|line| line.is_empty() || line.starts_with(' ') || line.starts_with('\t'))
            .map(|line| line.trim())
            .collect::<Vec<_>>()
            .join(" ")
    } else if let Some(quoted) = value.strip_prefix('\'') {
        let mut value = String::new();
        let mut chars = quoted.chars().peekable();
        loop {
            match chars.next()? {
                '\'' if chars.peek() == Some(&'\'') => {
                    chars.next();
                    value.push('\'');
                }
                '\'' => {
                    let tail: String = chars.collect();
                    let tail = tail.trim();
                    if !tail.is_empty() && !tail.starts_with('#') {
                        return None;
                    }
                    break value;
                }
                c => value.push(c),
            }
        }
    } else if value.starts_with('"') {
        // TOML basic strings share the common YAML double-quoted escapes.
        let doc: toml::Table = format!("description = {value}").parse().ok()?;
        doc.get("description")?.as_str()?.to_string()
    } else {
        if value.starts_with(['[', '{', '&', '*', '!', '|', '>']) {
            return None;
        }
        value
            .split_once(" #")
            .map_or(value, |(text, _)| text)
            .to_string()
    };
    let description = description.split_whitespace().collect::<Vec<_>>().join(" ");
    (!description.is_empty()).then_some(description)
}

/// Ignore hidden mirrors only when the same skill name exists outside hidden folders.
/// Unique hidden skills (for example .curated collections) remain discoverable.
pub fn prefer_visible_skills(found: &mut Vec<Candidate>) {
    let hidden = |c: &Candidate| c.subdir.split('/').any(|part| part.starts_with('.'));
    let visible: std::collections::BTreeSet<_> = found
        .iter()
        .filter(|c| !hidden(c))
        .map(|c| c.name.clone())
        .collect();
    found.retain(|c| !hidden(c) || !visible.contains(&c.name));
    found.sort_by(|a, b| a.subdir.cmp(&b.subdir));
}

/// Discover a local collection without following directory or document symlinks.
pub fn local_candidates(root: &Path) -> Result<Vec<Candidate>> {
    fn visit(root: &Path, dir: &Path, found: &mut Vec<Candidate>) -> Result<()> {
        let mut entries = fs::read_dir(dir)
            .with_context(|| format!("cannot read local collection {}", dir.display()))?
            .collect::<std::io::Result<Vec<_>>>()?;
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            let kind = entry.file_type()?;
            if kind.is_dir() && entry.file_name() != ".git" {
                visit(root, &entry.path(), found)?;
            } else if kind.is_file() && entry.file_name() == "SKILL.md" {
                let text = fs::read_to_string(entry.path())
                    .with_context(|| format!("cannot read {}", entry.path().display()))?;
                let subdir = dir
                    .strip_prefix(root)?
                    .to_str()
                    .context("local skill path is not UTF-8")?
                    .to_owned();
                let dir_name = dir
                    .file_name()
                    .and_then(|name| name.to_str())
                    .context("local skill directory name is not UTF-8")?
                    .to_owned();
                found.push(Candidate {
                    subdir,
                    name: frontmatter_name(&text).unwrap_or_else(|| dir_name.clone()),
                    dir_name,
                    description: frontmatter_description(&text),
                });
            }
        }
        Ok(())
    }
    let mut found = Vec::new();
    visit(root, root, &mut found)?;
    prefer_visible_skills(&mut found);
    Ok(found)
}
