//! User-facing source shorthand, normalized before entering the manifest.
use anyhow::{Result, bail};

#[derive(Debug)]
pub struct Source {
    pub repository: String,
    pub git_ref: Option<String>,
    pub subdir: Option<String>,
    pub skill: Option<String>,
    ambiguous_tail: Option<String>,
}

impl Source {
    pub fn resolve(&self, git: &crate::git::Git, url: &str) -> Result<String> {
        if let Some(tail) = &self.ambiguous_tail {
            git.check_tree_ref(url, tail)?;
        }
        git.resolve(url, self.git_ref.as_deref())
    }

    pub fn parse(input: &str, git_ref: Option<&str>, subdir: Option<&str>) -> Result<Self> {
        let mut result = Self {
            repository: input.into(),
            git_ref: git_ref.map(str::to_owned),
            subdir: subdir.map(str::to_owned),
            skill: None,
            ambiguous_tail: None,
        };
        // Only bare GitHub shorthand has an @skill suffix. Preserve SSH userinfo.
        if !input.contains(':')
            && !input.starts_with(['/', '.', '~'])
            && !std::path::Path::new(input).is_dir()
            && let Some((repo, skill)) = input.rsplit_once('@')
        {
            crate::manifest::source_url(repo)?;
            crate::manifest::validate_name(skill)?;
            result.repository = repo.into();
            result.skill = Some(skill.into());
        }
        if let Some(rest) = input
            .strip_prefix("https://")
            .or_else(|| input.strip_prefix("http://"))
            && let Some((host, path)) = rest.split_once('/')
        {
            let tree = if host == "github.com" {
                let parts: Vec<_> = path.splitn(4, '/').collect();
                if parts.len() == 4 && parts[2] == "tree" {
                    let tail = parts[3];
                    Some((&input[..input.len() - tail.len() - "/tree/".len()], tail))
                } else {
                    None
                }
            } else if path.contains("/-/tree/") {
                input.split_once("/-/tree/")
            } else {
                None
            };
            if let Some((base, tail)) = tree {
                if tail.contains(['?', '#', '%']) {
                    bail!(
                        "tree URLs with query strings, fragments or percent escapes are unsupported; use the repository URL with --ref and --subdir"
                    );
                }
                let tail = tail.trim_end_matches('/');
                if git_ref.is_none() && tail.contains('/') {
                    result.ambiguous_tail = Some(tail.into());
                }
                let (reference, directory) = if let Some(reference) = git_ref {
                    let directory = tail.strip_prefix(reference).and_then(|rest| {
                        if rest.is_empty() {
                            Some("")
                        } else {
                            rest.strip_prefix('/')
                        }
                    });
                    let Some(directory) = directory else {
                        bail!(
                            "--ref `{reference}` does not match the tree URL; use the repository URL to override its ref"
                        );
                    };
                    (reference, directory)
                } else {
                    tail.split_once('/').unwrap_or((tail, ""))
                };
                if reference.is_empty() {
                    bail!("tree URL requires a ref");
                }
                result.repository = base.into();
                result.git_ref = Some(reference.into());
                if !directory.is_empty() {
                    if let Some(explicit) = subdir
                        && explicit.trim_end_matches('/') != directory
                    {
                        bail!("--subdir conflicts with the tree URL path");
                    }
                    result.subdir = Some(directory.into());
                }
            }
        }
        if let Some(directory) = &result.subdir {
            let directory = directory.trim_end_matches('/');
            if directory.is_empty()
                || directory.starts_with('/')
                || directory
                    .split('/')
                    .any(|p| (p == "." && directory != ".") || p == ".." || p.is_empty())
            {
                bail!("--subdir must stay inside the repository");
            }
            result.subdir = Some(directory.into());
        }
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::Source;

    #[test]
    fn private_and_arbitrary_git_urls_are_preserved() {
        for url in [
            "git@github.com:owner/repo.git",
            "ssh://alice@git.example:2222/team/repo.git",
            "https://user@git.example/repo.git",
            "file:///tmp/repo",
            "https://example.com/owner/repo/tree/main",
        ] {
            let source = Source::parse(url, Some("feature/topic"), None).unwrap();
            assert_eq!(source.repository, url);
            assert_eq!(source.git_ref.as_deref(), Some("feature/topic"));
            assert!(source.skill.is_none());
            assert!(source.subdir.is_none());
        }
    }
}

#[cfg(test)]
#[test]
fn github_owner_named_tree_is_not_a_url_marker() {
    let source = Source::parse(
        "https://github.com/tree/catalog/tree/main/skills/one",
        None,
        None,
    )
    .unwrap();
    assert_eq!(source.repository, "https://github.com/tree/catalog");
    assert_eq!(source.subdir.as_deref(), Some("skills/one"));
}
