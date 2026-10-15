use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use toml::{Table, Value};

use crate::agents::{self, Agent};
use crate::paths::Paths;

/// The skills a machine should have, parsed from `skills.toml`.
///
/// ```toml
/// agents = ["claude-code", "codex"]
///
/// [skills]
/// notes = "example/notes"
/// review = { source = "example/agent-skills", ref = "v2.0.0" }
/// my-skill = { path = "local/my-skill" }
/// keyboard = { source = "example/keyboard", profiles = ["personal"] }
/// ```
pub struct Manifest {
    /// The agents skills link into unless they name their own.
    pub agents: Vec<Agent>,
    pub skills: Vec<Skill>,
    pub xcode: Option<Xcode>,
}

pub struct Xcode {
    /// None selects every exported skill.
    pub skills: Option<Vec<String>>,
    pub agents: Vec<Agent>,
    pub profiles: Vec<String>,
}

impl Xcode {
    pub fn selects(&self, name: &str) -> bool {
        self.skills
            .as_ref()
            .is_none_or(|names| names.iter().any(|n| n == name))
    }
}

pub struct Skill {
    pub name: String,
    pub origin: Origin,
    pub agents: Vec<Agent>,
    /// Profiles this skill installs under. Empty means every machine.
    pub profiles: Vec<String>,
}

pub enum Origin {
    Git(GitOrigin),
    Local(PathBuf),
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct GitOrigin {
    /// The source as written in the manifest, recorded in the lock.
    pub source: String,
    pub url: String,
    pub git_ref: Option<String>,
    /// Directory inside the repo holding SKILL.md. Discovered by name when unset.
    pub subdir: Option<String>,
}

pub fn load(paths: &Paths) -> Result<Manifest> {
    let path = paths.manifest();
    let text = fs::read_to_string(&path).with_context(|| {
        format!(
            "no manifest at {}: create it or run `skl add`",
            paths.tilde(&path)
        )
    })?;
    parse(&text, paths).with_context(|| format!("invalid manifest {}", paths.tilde(&path)))
}

pub fn parse(text: &str, paths: &Paths) -> Result<Manifest> {
    let mut table: Table = toml::from_str(text)?;
    let default_agents = match table.remove("agents") {
        Some(value) => agent_list(&value, "agents", paths)?,
        None => agents::DEFAULT
            .iter()
            .map(|id| agents::resolve(id, paths))
            .collect::<Result<_>>()?,
    };
    let xcode = table
        .remove("xcode")
        .map(|value| -> Result<Xcode> {
            let Value::Table(mut config) = value else {
                bail!("`xcode` must be a table")
            };
            let skills = match config.remove("skills") {
                Some(Value::String(s)) if s == "*" => None,
                Some(Value::Array(items)) => {
                    let mut names = Vec::new();
                    for item in items {
                        let Value::String(name) = item else {
                            bail!("`xcode.skills` must contain names")
                        };
                        validate_name(&name)?;
                        if !names.contains(&name) {
                            names.push(name);
                        }
                    }
                    Some(names)
                }
                _ => bail!("set `xcode.skills` to `\"*\"` or an array of names"),
            };
            if let Some(key) = config
                .keys()
                .find(|k| !matches!(k.as_str(), "agents" | "profiles"))
            {
                bail!("unknown xcode key `{key}`");
            }
            // Reuse per-skill agents and profiles validation.
            config.insert("path".into(), Value::String(".".into()));
            let options = parse_skill("xcode", Value::Table(config), &default_agents, paths)?;
            Ok(Xcode {
                skills,
                agents: options.agents,
                profiles: options.profiles,
            })
        })
        .transpose()?;
    let skills = match table.remove("skills") {
        Some(Value::Table(skills)) => skills,
        Some(_) => bail!("`skills` must be a table"),
        None => Table::new(),
    };
    if let Some(key) = table.keys().next() {
        bail!("unknown key `{key}`");
    }

    let mut parsed = Vec::with_capacity(skills.len());
    for (name, value) in skills {
        validate_name(&name)?;
        let skill = parse_skill(&name, value, &default_agents, paths)
            .with_context(|| format!("skill `{name}`"))?;
        parsed.push(skill);
    }
    Ok(Manifest {
        agents: default_agents,
        skills: parsed,
        xcode,
    })
}

fn parse_skill(name: &str, value: Value, default_agents: &[Agent], paths: &Paths) -> Result<Skill> {
    let mut table = match value {
        Value::String(source) => {
            let origin = git_origin(source, None, None)?;
            return Ok(Skill {
                name: name.into(),
                origin,
                agents: default_agents.to_vec(),
                profiles: Vec::new(),
            });
        }
        Value::Table(table) => table,
        _ => bail!("expected a source string or a table"),
    };

    let mut take_str = |key: &str| -> Result<Option<String>> {
        match table.remove(key) {
            None => Ok(None),
            Some(Value::String(s)) if !s.is_empty() => Ok(Some(s)),
            Some(_) => bail!("`{key}` must be a non-empty string"),
        }
    };
    let source = take_str("source")?;
    let local = take_str("path")?;
    let git_ref = take_str("ref")?;
    let subdir = take_str("subdir")?;
    let agents = match table.remove("agents") {
        Some(value) => agent_list(&value, "agents", paths)?,
        None => default_agents.to_vec(),
    };
    let profiles = match table.remove("profiles") {
        None => Vec::new(),
        Some(Value::Array(items)) if !items.is_empty() => {
            let mut profiles = Vec::with_capacity(items.len());
            for item in items {
                let Value::String(name) = item else {
                    bail!("`profiles` must be an array of strings")
                };
                crate::profile::validate(&name)?;
                profiles.push(name);
            }
            profiles
        }
        Some(_) => bail!("`profiles` must be a non-empty array of strings"),
    };
    if let Some(key) = table.keys().next() {
        bail!("unknown key `{key}`");
    }

    let origin = match (source, local) {
        (Some(source), None) => git_origin(source, git_ref, subdir)?,
        (None, Some(local)) => {
            if git_ref.is_some() || subdir.is_some() {
                bail!("`ref` and `subdir` only apply to `source`");
            }
            Origin::Local(paths.expand(&local))
        }
        _ => bail!("set exactly one of `source` or `path`"),
    };
    Ok(Skill {
        name: name.into(),
        origin,
        agents,
        profiles,
    })
}

fn git_origin(source: String, git_ref: Option<String>, subdir: Option<String>) -> Result<Origin> {
    let url = source_url(&source)?;
    let subdir = subdir
        .map(|s| s.trim_matches('/').to_string())
        .filter(|s| !s.is_empty());
    if let Some(dir) = &subdir
        && dir.split('/').any(|part| part == ".." || part == ".")
    {
        bail!("`subdir` must stay inside the repository");
    }
    Ok(Origin::Git(GitOrigin {
        source,
        url,
        git_ref,
        subdir,
    }))
}

/// `owner/repo` means GitHub. URLs, scp-style addresses and absolute paths pass through.
pub fn source_url(source: &str) -> Result<String> {
    if source.contains("://") || source.starts_with("git@") || source.starts_with('/') {
        return Ok(source.into());
    }
    let parts: Vec<_> = source.split('/').collect();
    let valid = |part: &&str| {
        !part.is_empty()
            && part
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c))
    };
    if parts.len() == 2 && parts.iter().all(valid) {
        return Ok(format!("https://github.com/{source}.git"));
    }
    bail!("`{source}` is not owner/repo, a git URL, or an absolute path")
}

pub fn validate_name(name: &str) -> Result<()> {
    let ok = !name.is_empty()
        && !name.starts_with('.')
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c));
    if !ok {
        bail!("`{name}` is not a valid skill name: use letters, digits, `-`, `_` and `.`");
    }
    Ok(())
}

fn agent_list(value: &Value, key: &str, paths: &Paths) -> Result<Vec<Agent>> {
    let Value::Array(items) = value else {
        bail!("`{key}` must be an array of strings")
    };
    let mut agents = Vec::with_capacity(items.len());
    for item in items {
        let Value::String(id) = item else {
            bail!("`{key}` must be an array of strings")
        };
        let agent = agents::resolve(id, paths)?;
        if !agents.contains(&agent) {
            agents.push(agent);
        }
    }
    Ok(agents)
}
