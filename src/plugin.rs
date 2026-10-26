//! Claude Code and Cursor plugins keep skills next to agents, commands and hooks.
//! skl installs only the skills, so `add` and `discover` point out the rest.
use std::collections::BTreeSet;
use std::path::Path;

use crate::git::Git;

/// The manifests that mark a plugin's root directory.
const MANIFESTS: [&str; 2] = [".claude-plugin/plugin.json", ".cursor-plugin/plugin.json"];

pub struct Plugin {
    /// The plugin's directory in the repository, empty for the root.
    pub root: String,
    pub name: String,
    pub agents: Vec<String>,
    pub commands: bool,
    pub hooks: bool,
    /// The repository marketplace that lists this plugin, by its name.
    pub marketplace: Option<String>,
}

impl Plugin {
    /// Whether the plugin ships anything besides skills.
    pub fn has_more(&self) -> bool {
        !self.agents.is_empty() || self.commands || self.hooks
    }

    /// What the plugin ships besides skills, such as `agents a, b and commands`.
    pub fn extras(&self) -> String {
        let mut parts = Vec::new();
        if !self.agents.is_empty() {
            parts.push(format!("agents {}", self.agents.join(", ")));
        }
        if self.commands {
            parts.push("commands".into());
        }
        if self.hooks {
            parts.push("hooks".into());
        }
        match parts.split_last() {
            Some((last, [])) => last.clone(),
            Some((last, rest)) => format!("{} and {last}", rest.join(", ")),
            None => String::new(),
        }
    }

    /// Tells the user that the plugin's agents, commands or hooks stay behind, and
    /// how to install the whole plugin instead.
    pub fn warn(&self, source: &str) {
        let at = if self.root.is_empty() {
            "."
        } else {
            &self.root
        };
        eprintln!(
            "warning: plugin `{}` ({at}) also ships {}, which installing its skills alone leaves out. Skills that hand work to them won't find them.",
            self.name,
            self.extras()
        );
        eprintln!(
            "  Install the whole plugin instead: {}",
            self.command(source)
        );
    }

    /// The `skl add` command that installs this plugin.
    pub fn command(&self, source: &str) -> String {
        match &self.marketplace {
            Some(_) => format!("skl add {source} --plugin {}", self.name),
            None => {
                let at = if self.root.is_empty() {
                    "."
                } else {
                    &self.root
                };
                format!("skl add {source} --plugin {} --subdir {at}", self.name)
            }
        }
    }
}

/// The nearest plugin containing each directory, once per plugin.
pub fn containing(
    git: &Git,
    repo: &Path,
    commit: &str,
    files: &[String],
    dirs: &[&str],
) -> Vec<Plugin> {
    let set: BTreeSet<&str> = files.iter().map(String::as_str).collect();
    let mut roots = BTreeSet::new();
    for dir in dirs {
        let mut current = Some(*dir);
        while let Some(dir) = current {
            if MANIFESTS
                .iter()
                .any(|m| set.contains(join(dir, m).as_str()))
            {
                roots.insert(dir.to_owned());
                break;
            }
            current = match dir.rsplit_once('/') {
                Some((parent, _)) => Some(parent),
                None if !dir.is_empty() => Some(""),
                None => None,
            };
        }
    }
    let marketplace = read_json(git, repo, commit, &set, ".claude-plugin/marketplace.json");
    roots
        .into_iter()
        .map(|root| {
            let name = MANIFESTS
                .iter()
                .find_map(|m| {
                    let manifest = read_json(git, repo, commit, &set, &join(&root, m))?;
                    Some(manifest.get("name")?.as_str()?.to_owned())
                })
                .unwrap_or_else(|| root.rsplit('/').next().unwrap_or_default().to_owned());
            let inside = |sub: &str| join(&root, sub) + "/";
            let agents = inside("agents");
            let agents = files
                .iter()
                .filter_map(|f| f.strip_prefix(&agents)?.strip_suffix(".md"))
                .filter(|stem| !stem.contains('/'))
                .map(str::to_owned)
                .collect();
            let commands = inside("commands");
            let listed = marketplace.as_ref().and_then(|m| {
                let entries = m.get("plugins")?.as_array()?;
                entries
                    .iter()
                    .any(|p| {
                        p.get("name").and_then(|n| n.as_str()) == Some(name.as_str())
                            && p.get("source").and_then(|s| s.as_str()).is_some_and(|s| {
                                s.trim_start_matches("./").trim_end_matches('/') == root
                            })
                    })
                    .then(|| m.get("name")?.as_str().map(str::to_owned))?
            });
            Plugin {
                agents,
                commands: files.iter().any(|f| f.starts_with(&commands)),
                hooks: set.contains(join(&root, "hooks/hooks.json").as_str()),
                marketplace: listed,
                name,
                root,
            }
        })
        .collect()
}

fn join(dir: &str, file: &str) -> String {
    if dir.is_empty() {
        file.to_owned()
    } else {
        format!("{dir}/{file}")
    }
}

fn read_json(
    git: &Git,
    repo: &Path,
    commit: &str,
    files: &BTreeSet<&str>,
    file: &str,
) -> Option<serde_json::Value> {
    if !files.contains(file) {
        return None;
    }
    serde_json::from_str(&git.read(repo, commit, file).ok()?).ok()
}
