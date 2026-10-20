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
    /// Git repositories selecting `"*"`, expanded once their membership is known.
    pub repositories: std::collections::BTreeMap<String, crate::repositories::Repository>,
    /// Repositories whose skills an installed app exports, including Xcode.
    pub apps: std::collections::BTreeMap<String, crate::export::App>,
}

pub fn matches_os(os: &[String]) -> bool {
    os.is_empty() || os.iter().any(|name| name == std::env::consts::OS)
}

pub struct Skill {
    pub name: String,
    pub origin: Origin,
    pub agents: Vec<Agent>,
    /// Profiles this skill installs under. Empty means every machine.
    pub profiles: Vec<String>,
    pub os: Vec<String>,
}

impl Skill {
    pub fn enabled(&self, active: &crate::profile::Active) -> bool {
        matches_os(&self.os) && active.includes(&self.profiles)
    }
}

pub enum Origin {
    Git(GitOrigin),
    Local(PathBuf),
    OptionalLocal(PathBuf),
    Command(CommandOrigin),
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CommandOrigin {
    pub command: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version_command: Option<Vec<String>>,
    pub timeout: u64,
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
    // `[xcode]` is shorthand for the Xcode app source.
    if let Some(value) = table.remove("xcode") {
        let Value::Table(mut config) = value else {
            bail!("`xcode` must be a table")
        };
        if !matches!(
            config.get("skills"),
            Some(Value::String(_) | Value::Array(_))
        ) {
            bail!("set `xcode.skills` to `\"*\"` or an array of names");
        }
        if let Some(key) = config.keys().find(|k| {
            !matches!(
                k.as_str(),
                "skills" | "exclude" | "agents" | "profiles" | "os"
            )
        }) {
            bail!("unknown xcode key `{key}`");
        }
        config.insert("app".into(), Value::String(crate::export::XCODE.into()));
        for key in crate::repositories::TABLES {
            if let Some(Value::Table(declared)) = table.get(key)
                && declared.contains_key(crate::export::XCODE)
            {
                bail!("declare Xcode once: `[xcode]` and `[{key}.xcode]` both exist");
            }
        }
        let declarations = table
            .entry("sources")
            .or_insert_with(|| Value::Table(Table::new()));
        let Value::Table(declarations) = declarations else {
            bail!("`sources` must be a table")
        };
        declarations.insert(crate::export::XCODE.into(), Value::Table(config));
    }
    let (repositories, apps) = crate::repositories::parse(&mut table, &default_agents, paths)?;
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
        repositories,
        apps,
    })
}

pub(crate) fn parse_skill(
    name: &str,
    value: Value,
    default_agents: &[Agent],
    paths: &Paths,
) -> Result<Skill> {
    let mut table = match value {
        Value::String(source) => {
            let origin = git_origin(source, None, None)?;
            return Ok(Skill {
                name: name.into(),
                origin,
                agents: default_agents.to_vec(),
                profiles: Vec::new(),
                os: Vec::new(),
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
    let command = argv(table.remove("command"), "command")?;
    let version_command = argv(table.remove("version-command"), "version-command")?;
    let has_timeout = table.contains_key("timeout");
    let timeout = match table.remove("timeout") {
        None => 30,
        Some(Value::Integer(n)) if (1..=300).contains(&n) => n as u64,
        Some(_) => bail!("timeout must be an integer from 1 to 300 seconds"),
    };
    let optional = match table.remove("optional") {
        None => false,
        Some(Value::Boolean(value)) if local.is_some() => value,
        Some(_) => bail!("optional is a boolean for path skills only"),
    };
    if command.is_none() && (version_command.is_some() || has_timeout) {
        bail!("version-command and timeout require command");
    }
    let agents = match table.remove("agents") {
        Some(value) => agent_list(&value, "agents", paths)?,
        None => default_agents.to_vec(),
    };
    let os = match table.remove("os") {
        None => Vec::new(),
        Some(Value::Array(items)) if !items.is_empty() => {
            let mut names = Vec::new();
            for item in items {
                let Value::String(name) = item else {
                    bail!("`os` must be an array of strings")
                };
                if !matches!(name.as_str(), "macos" | "linux") {
                    bail!("unknown operating system `{name}`: use `macos` or `linux`");
                }
                if !names.contains(&name) {
                    names.push(name);
                }
            }
            names
        }
        Some(_) => bail!("`os` must be a non-empty array of strings"),
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

    let origin = match (source, local, command) {
        (Some(source), None, None) => git_origin(source, git_ref, subdir)?,
        (None, Some(local), None) => {
            if git_ref.is_some() || subdir.is_some() {
                bail!("`ref` and `subdir` only apply to `source`");
            }
            if optional {
                Origin::OptionalLocal(paths.expand(&local))
            } else {
                Origin::Local(paths.expand(&local))
            }
        }
        (None, None, Some(command)) => {
            if git_ref.is_some() || subdir.is_some() {
                bail!("ref and subdir only apply to Git sources");
            }
            Origin::Command(CommandOrigin {
                command,
                version_command,
                timeout,
            })
        }
        _ => bail!("set exactly one of `source`, `path` or `command`"),
    };
    Ok(Skill {
        name: name.into(),
        origin,
        agents,
        profiles,
        os,
    })
}

fn git_origin(source: String, git_ref: Option<String>, subdir: Option<String>) -> Result<Origin> {
    let url = source_url(&source)?;
    let subdir = subdir.and_then(|s| {
        if s == "." {
            Some(String::new())
        } else {
            let s = s.trim_matches('/');
            (!s.is_empty()).then(|| s.to_string())
        }
    });
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

fn argv(value: Option<Value>, key: &str) -> Result<Option<Vec<String>>> {
    let Some(value) = value else { return Ok(None) };
    let Value::Array(items) = value else {
        bail!("{key} must be an argv array")
    };
    let args = items
        .into_iter()
        .map(|v| match v {
            Value::String(s) if !s.contains('\0') => Ok(s),
            _ => bail!("{key} must contain strings without NUL bytes"),
        })
        .collect::<Result<Vec<_>>>()?;
    if args.first().is_none_or(|s| s.is_empty() || s == "{output}") {
        bail!("{key} requires an executable");
    }
    if args[0].contains('/') && !std::path::Path::new(&args[0]).is_absolute() {
        bail!("{key} executable must be absolute or a name on PATH");
    }
    if key == "version-command" && args.iter().any(|s| s.contains("{output}")) {
        bail!("version-command cannot use {{output}}");
    }
    if args
        .iter()
        .any(|s| s.contains("{output}") && s != "{output}")
    {
        bail!("{{output}} must occupy a whole argument");
    }
    Ok(Some(args))
}
