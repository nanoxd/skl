//! Claude Code and Codex plugins: skills together with agents, commands, hooks and
//! MCP servers. skl pins each plugin to a commit and unpacks it into the store like
//! a skill. Each harness then installs it with its own CLI from a marketplace that
//! skl generates under `<data>/plugins/<agent>`, one symlink per plugin into the
//! store. Claude Code reads a local marketplace's plugins in place, so a new commit
//! is a repointed symlink. Codex copies them, so skl adds the plugin again.
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{ErrorKind, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use toml::{Table, Value};

use crate::agents::Agent;
use crate::git::Git;
use crate::lock::LockedPlugin;
use crate::manifest::{self, Origin};
use crate::paths::Paths;

/// The marketplace name skl registers with each harness, so plugins are `NAME@skl`.
pub const MARKETPLACE: &str = "skl";

/// Agents that install plugins, and how skl drives each one.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Harness {
    ClaudeCode,
    Codex,
}

impl Harness {
    pub const ALL: [Self; 2] = [Self::ClaudeCode, Self::Codex];

    pub fn id(self) -> &'static str {
        match self {
            Self::ClaudeCode => "claude-code",
            Self::Codex => "codex",
        }
    }

    pub fn named(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|h| h.id() == id)
    }

    /// The CLI skl installs through.
    fn program(self) -> &'static str {
        match self {
            Self::ClaudeCode => "claude",
            Self::Codex => "codex",
        }
    }

    /// Whether this machine has the agent's CLI, so its plugins can be installed.
    pub fn available(self) -> Result<bool> {
        Ok(crate::app::executable(self.program())?.is_some())
    }

    /// Why this agent's plugins are skipped when it isn't available.
    pub fn absence(self) -> String {
        format!("`{}` isn't installed", self.program())
    }

    /// Where this harness looks for a repository's marketplace, in order.
    fn marketplaces(self) -> &'static [&'static str] {
        match self {
            Self::ClaudeCode => &[".claude-plugin/marketplace.json"],
            Self::Codex => &[
                ".agents/plugins/marketplace.json",
                ".claude-plugin/marketplace.json",
            ],
        }
    }

    /// The plugin manifests this harness reads, in order.
    fn manifests(self) -> &'static [&'static str] {
        match self {
            Self::ClaudeCode => &[".claude-plugin/plugin.json"],
            Self::Codex => &[
                ".codex-plugin/plugin.json",
                ".claude-plugin/plugin.json",
                ".cursor-plugin/plugin.json",
            ],
        }
    }

    /// Claude Code also accepts a plugin with no manifest, made of its usual folders.
    fn accepts(self, files: &BTreeSet<&str>, dir: &str) -> bool {
        let inside = |path: &str| join(dir, path);
        if self
            .manifests()
            .iter()
            .any(|m| files.contains(inside(m).as_str()))
        {
            return true;
        }
        self == Self::ClaudeCode
            && ["skills/", "agents/", "commands/"].iter().any(|folder| {
                let prefix = inside(folder);
                files.iter().any(|f| f.starts_with(&prefix))
            })
    }

    fn register(self, marketplace: &Path) -> Vec<String> {
        let path = marketplace.to_string_lossy().into_owned();
        args(&["plugin", "marketplace", "add", &path, "--json"])
    }

    fn list_marketplaces(self) -> Vec<String> {
        args(&["plugin", "marketplace", "list", "--json"])
    }

    fn list(self) -> Vec<String> {
        args(&["plugin", "list", "--json"])
    }

    fn install(self, id: &str) -> Vec<String> {
        match self {
            Self::ClaudeCode => args(&["plugin", "install", id, "--scope", "user", "--json"]),
            Self::Codex => args(&["plugin", "add", id, "--json"]),
        }
    }

    fn uninstall(self, id: &str) -> Vec<String> {
        match self {
            Self::ClaudeCode => args(&["plugin", "uninstall", id, "--scope", "user", "--json"]),
            Self::Codex => args(&["plugin", "remove", id, "--json"]),
        }
    }

    /// How the user loads plugin changes.
    fn reload(self) -> &'static str {
        match self {
            Self::ClaudeCode => "restart Claude Code or run /reload-plugins",
            Self::Codex => "restart Codex",
        }
    }

    /// Whether a new commit needs the plugin installed again. Claude Code reads a
    /// local marketplace's plugin folder in place; Codex keeps a copy.
    fn copies(self) -> bool {
        self == Self::Codex
    }

    /// The registered path of the marketplace named `name`, if any.
    fn marketplace_path(self, output: &str, name: &str) -> Result<Option<PathBuf>> {
        let value: serde_json::Value = parse_json(output)?;
        let entries = match self {
            Self::ClaudeCode => value.as_array().cloned().unwrap_or_default(),
            Self::Codex => value
                .get("marketplaces")
                .and_then(|m| m.as_array())
                .cloned()
                .unwrap_or_default(),
        };
        Ok(entries.iter().find_map(|entry| {
            if entry.get("name")?.as_str()? != name {
                return None;
            }
            let key = match self {
                Self::ClaudeCode => "path",
                Self::Codex => "root",
            };
            Some(PathBuf::from(entry.get(key)?.as_str()?))
        }))
    }

    /// Installed plugin ids, such as `pstack@skl`.
    fn installed(self, output: &str) -> Result<BTreeSet<String>> {
        let value: serde_json::Value = parse_json(output)?;
        let (entries, key) = match self {
            Self::ClaudeCode => (value.as_array().cloned().unwrap_or_default(), "id"),
            Self::Codex => (
                value
                    .get("installed")
                    .and_then(|i| i.as_array())
                    .cloned()
                    .unwrap_or_default(),
                "pluginId",
            ),
        };
        Ok(entries
            .iter()
            .filter_map(|e| Some(e.get(key)?.as_str()?.to_owned()))
            .collect())
    }
}

fn args(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| s.to_string()).collect()
}

/// The last JSON document a CLI printed; some print progress lines first.
fn parse_json(output: &str) -> Result<serde_json::Value> {
    if let Ok(value) = serde_json::from_str(output.trim()) {
        return Ok(value);
    }
    let start = output.find(['[', '{']).context("expected JSON output")?;
    serde_json::from_str(output[start..].trim()).context("expected JSON output")
}

/// A `[plugins.<name>]` entry.
pub struct Plugin {
    /// The manifest key.
    pub name: String,
    pub source: String,
    pub url: String,
    pub git_ref: Option<String>,
    /// The plugin's name in the repository's marketplace.
    pub plugin: String,
    /// Folders set in the manifest: one for every agent, or one per agent.
    pub subdir: Subdir,
    pub agents: Vec<Harness>,
    pub profiles: Vec<String>,
    pub os: Vec<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Subdir {
    /// Found through each harness's marketplace file.
    Marketplace,
    All(String),
    PerAgent(BTreeMap<Harness, String>),
}

impl Subdir {
    fn get(&self, harness: Harness) -> Option<&str> {
        match self {
            Self::Marketplace => None,
            Self::All(dir) => Some(dir),
            Self::PerAgent(dirs) => dirs.get(&harness).map(String::as_str),
        }
    }
}

impl Plugin {
    pub fn enabled(&self, active: &crate::profile::Active) -> bool {
        manifest::matches_os(&self.os) && active.includes(&self.profiles)
    }
}

/// Parses `[plugins]`. Agents default to the manifest's agents that install plugins.
pub fn parse(value: Option<Value>, default_agents: &[Agent], paths: &Paths) -> Result<Vec<Plugin>> {
    let table = match value {
        None => return Ok(Vec::new()),
        Some(Value::Table(table)) => table,
        Some(_) => bail!("`plugins` must be a table"),
    };
    if paths.project.is_some() && !table.is_empty() {
        bail!("plugins install for your user account: declare them in the global manifest");
    }
    let mut plugins = Vec::new();
    for (name, value) in table {
        manifest::validate_name(&name)?;
        let plugin = parse_one(&name, value, default_agents, paths)
            .with_context(|| format!("plugin `{name}`"))?;
        plugins.push(plugin);
    }
    Ok(plugins)
}

pub fn parse_one(
    name: &str,
    value: Value,
    default_agents: &[Agent],
    paths: &Paths,
) -> Result<Plugin> {
    let mut table = match value {
        Value::String(source) => {
            let mut table = Table::new();
            table.insert("source".into(), Value::String(source));
            table
        }
        Value::Table(table) => table,
        _ => bail!("expected a source string or a table"),
    };
    if let Some(key) = table.keys().find(|k| {
        !matches!(
            k.as_str(),
            "source" | "ref" | "plugin" | "subdir" | "agents" | "profiles" | "os"
        )
    }) {
        bail!(
            "unknown key `{key}`: plugins support source, ref, plugin, subdir, agents, profiles, and os"
        );
    }
    let plugin = match table.remove("plugin") {
        None => name.to_owned(),
        Some(Value::String(plugin)) if !plugin.is_empty() => plugin,
        Some(_) => bail!("`plugin` must be a non-empty string"),
    };
    let directory = |dir: &str| -> Result<String> {
        if dir == "." {
            return Ok(String::new());
        }
        Ok(manifest::directory(dir)?.unwrap_or_default())
    };
    let subdir = match table.remove("subdir") {
        None => Subdir::Marketplace,
        Some(Value::String(dir)) => Subdir::All(directory(&dir)?),
        Some(Value::Table(dirs)) => {
            let mut parsed = BTreeMap::new();
            for (agent, dir) in dirs {
                let harness = Harness::named(&agent).with_context(|| {
                    format!("`subdir` names `{agent}`, which doesn't install plugins: use claude-code or codex")
                })?;
                let Value::String(dir) = dir else {
                    bail!("`subdir.{agent}` must be a string");
                };
                parsed.insert(harness, directory(&dir)?);
            }
            Subdir::PerAgent(parsed)
        }
        Some(_) => bail!("`subdir` must be a string or a table of agents"),
    };
    let agents = match table.remove("agents") {
        Some(Value::Array(items)) if !items.is_empty() => {
            let mut agents = Vec::new();
            for item in items {
                let Value::String(id) = item else {
                    bail!("`agents` must be an array of strings");
                };
                let harness = Harness::named(&id).with_context(|| {
                    format!("`{id}` doesn't install plugins: use claude-code or codex")
                })?;
                if !agents.contains(&harness) {
                    agents.push(harness);
                }
            }
            agents
        }
        Some(_) => bail!("`agents` must be a non-empty array of strings"),
        None => {
            let agents: Vec<_> = default_agents
                .iter()
                .filter_map(|agent| Harness::named(&agent.id))
                .collect();
            if agents.is_empty() {
                bail!(
                    "none of the manifest's agents install plugins: set agents to claude-code or codex"
                );
            }
            agents
        }
    };
    // Source, ref, profiles and os mean what they do for a skill.
    let skill = manifest::parse_skill(name, Value::Table(table), default_agents, paths)?;
    let Origin::Git(origin) = skill.origin else {
        bail!("set `source` to a Git repository");
    };
    Ok(Plugin {
        name: name.to_owned(),
        source: origin.source,
        url: origin.url,
        git_ref: origin.git_ref,
        plugin,
        subdir,
        agents,
        profiles: skill.profiles,
        os: skill.os,
    })
}

impl LockedPlugin {
    /// Whether this entry still pins what the manifest asks for, so syncing needs
    /// neither the remote's newest commit nor any lookup in the repository.
    pub fn matches(&self, plugin: &Plugin) -> bool {
        self.same_source(plugin)
            && plugin.agents.iter().all(|harness| {
                match (plugin.subdir.get(*harness), self.subdirs.get(harness.id())) {
                    (Some(wanted), Some(locked)) => wanted == locked,
                    (None, locked) => locked.is_some(),
                    (Some(_), None) => false,
                }
            })
    }

    /// Whether the commit still follows the manifest's source and ref.
    fn same_source(&self, plugin: &Plugin) -> bool {
        self.source == plugin.source
            && self.git_ref == plugin.git_ref
            && self.plugin == plugin.plugin
    }
}

/// Checks that every agent can find the plugin at the newest commit of its ref,
/// returning each agent's folder.
pub fn check(paths: &Paths, plugin: &Plugin) -> Result<BTreeMap<String, String>> {
    let git = Git::new(paths.repos());
    let commit = git.resolve(&plugin.url, plugin.git_ref.as_deref())?;
    let repo = git.ensure_commit(&plugin.url, &commit)?;
    locate(&git, &repo, &commit, plugin)
}

/// The plugin folder each agent installs, at one commit.
fn locate(
    git: &Git,
    repo: &Path,
    commit: &str,
    plugin: &Plugin,
) -> Result<BTreeMap<String, String>> {
    let files = git.files(repo, commit)?;
    let set: BTreeSet<&str> = files.iter().map(String::as_str).collect();
    let mut dirs = BTreeMap::new();
    for harness in &plugin.agents {
        let dir = match plugin.subdir.get(*harness) {
            Some(dir) => dir.to_owned(),
            None => from_marketplace(git, repo, commit, &set, *harness, &plugin.plugin)?
                .with_context(|| {
                    format!(
                        "no marketplace in {} lists plugin `{}` for {}: set `subdir` to its folder",
                        plugin.source,
                        plugin.plugin,
                        harness.id()
                    )
                })?,
        };
        if !harness.accepts(&set, &dir) {
            let at = if dir.is_empty() { "." } else { &dir };
            bail!(
                "{} has no plugin {} reads in `{at}`: it needs {}",
                plugin.source,
                harness.id(),
                harness.manifests().join(" or ")
            );
        }
        dirs.insert(harness.id().to_owned(), dir);
    }
    Ok(dirs)
}

/// The folder a harness's marketplace file gives for `name`, if it lists it.
fn from_marketplace(
    git: &Git,
    repo: &Path,
    commit: &str,
    files: &BTreeSet<&str>,
    harness: Harness,
    name: &str,
) -> Result<Option<String>> {
    for file in harness.marketplaces() {
        if !files.contains(file) {
            continue;
        }
        let text = git.read(repo, commit, file)?;
        let value: serde_json::Value =
            serde_json::from_str(&text).with_context(|| format!("invalid {file}"))?;
        let root = value
            .get("metadata")
            .and_then(|m| m.get("pluginRoot"))
            .and_then(|r| r.as_str())
            .unwrap_or("");
        let Some(entries) = value.get("plugins").and_then(|p| p.as_array()) else {
            continue;
        };
        let Some(entry) = entries
            .iter()
            .find(|e| e.get("name").and_then(|n| n.as_str()) == Some(name))
        else {
            continue;
        };
        let source = match entry.get("source") {
            Some(serde_json::Value::String(source)) => source.clone(),
            Some(serde_json::Value::Object(source))
                if source.get("source").and_then(|s| s.as_str()) == Some("local") =>
            {
                source
                    .get("path")
                    .and_then(|p| p.as_str())
                    .unwrap_or_default()
                    .to_owned()
            }
            _ => bail!(
                "{file} lists `{name}` from another repository, which skl doesn't follow yet: set `source` to that repository"
            ),
        };
        let relative = match source.strip_prefix("./") {
            Some(rest) => rest.to_owned(),
            None if source == "." => String::new(),
            None => join(root.trim_start_matches("./").trim_end_matches('/'), &source),
        };
        let dir = relative.trim_end_matches('/');
        if dir.is_empty() {
            return Ok(Some(String::new()));
        }
        return Ok(Some(
            manifest::directory(dir)
                .with_context(|| format!("{file} lists `{name}` outside the repository"))?
                .unwrap_or_default(),
        ));
    }
    Ok(None)
}

fn join(dir: &str, file: &str) -> String {
    if dir.is_empty() {
        file.to_owned()
    } else {
        format!("{dir}/{file}")
    }
}

/// Where a plugin's folder is unpacked, shared with skills from the same tree.
pub fn store_dir(paths: &Paths, url: &str, commit: &str, subdir: &str) -> PathBuf {
    paths
        .store()
        .join(crate::git::cache_key(url))
        .join(commit)
        .join(crate::sync::store_name(subdir))
}

/// What happened while resolving plugins, for the sync report.
pub enum Event {
    Added(String, String),
    Moved(String, String, String),
    Diff(String),
    Error(String, String),
}

/// Builds each plugin's lock entry, fetching only what changed, and unpacks new
/// folders into the store unless this is a dry run. A plugin that fails keeps its
/// previous entry. A saved plan supplies the commits instead of the remote.
pub fn resolve(
    paths: &Paths,
    git: &Git,
    plugins: &[Plugin],
    old: &BTreeMap<String, LockedPlugin>,
    planned: Option<&BTreeMap<String, LockedPlugin>>,
    active: &crate::profile::Active,
    opts: &crate::sync::Options,
) -> Result<(BTreeMap<String, LockedPlugin>, Vec<Event>)> {
    let (update, locked, dry_run, patch) =
        (opts.update.as_ref(), opts.locked, opts.dry_run, opts.patch);
    let mut entries = BTreeMap::new();
    let mut events = Vec::new();
    let wants_update = |name: &str| {
        update.is_some_and(|names| names.is_empty() || names.iter().any(|n| n == name))
    };
    for plugin in plugins {
        let previous = old.get(&plugin.name);
        if !plugin.enabled(active) {
            if let Some(entry) = previous {
                entries.insert(plugin.name.clone(), entry.clone());
            }
            continue;
        }
        let refresh =
            wants_update(&plugin.name) || !previous.is_some_and(|e| e.same_source(plugin));
        if !refresh && previous.is_some_and(|e| e.matches(plugin)) {
            let entry = previous.unwrap().clone();
            if !dry_run {
                for dir in entry.subdirs.values() {
                    if let Err(err) = ensure_unpacked(paths, git, plugin, &entry.commit, dir) {
                        events.push(Event::Error(plugin.name.clone(), format!("{err:#}")));
                    }
                }
            }
            entries.insert(plugin.name.clone(), entry);
            continue;
        }
        if locked {
            bail!(
                "skills.lock is out of date for plugin `{}`: run `skl sync` without --locked",
                plugin.name
            );
        }
        let result = (|| -> Result<LockedPlugin> {
            let commit = match (planned.and_then(|p| p.get(&plugin.name)), previous) {
                (Some(entry), _) => entry.commit.clone(),
                (None, Some(entry)) if !refresh => entry.commit.clone(),
                _ => git.resolve(&plugin.url, plugin.git_ref.as_deref())?,
            };
            let repo = git.ensure_commit(&plugin.url, &commit)?;
            let subdirs = locate(git, &repo, &commit, plugin)?;
            let entry = LockedPlugin {
                source: plugin.source.clone(),
                git_ref: plugin.git_ref.clone(),
                plugin: plugin.plugin.clone(),
                commit,
                subdirs,
            };
            if !dry_run {
                for dir in entry.subdirs.values() {
                    ensure_unpacked(paths, git, plugin, &entry.commit, dir)?;
                }
            }
            if let Some(previous) = previous.filter(|p| p.commit != entry.commit) {
                let mut seen = BTreeSet::new();
                for dir in entry.subdirs.values().filter(|d| seen.insert(*d)) {
                    let old_dir = previous.subdirs.values().find(|d| *d == dir).unwrap_or(dir);
                    let tree = |commit: &str, dir: &str| {
                        if dir.is_empty() {
                            commit.to_owned()
                        } else {
                            format!("{commit}:{dir}")
                        }
                    };
                    // The old commit may be gone from a fresh cache; skip the diff then.
                    if git.ensure_commit(&plugin.url, &previous.commit).is_ok()
                        && let Ok(text) = git.diff(
                            &repo,
                            &tree(&previous.commit, old_dir),
                            &tree(&entry.commit, dir),
                            patch,
                        )
                        && !text.trim().is_empty()
                    {
                        events.push(Event::Diff(text));
                    }
                }
            }
            Ok(entry)
        })();
        match result {
            Ok(entry) => {
                let short = &entry.commit[..7.min(entry.commit.len())];
                match previous {
                    None => events.push(Event::Added(
                        plugin.name.clone(),
                        format!("plugin {} @ {short}", plugin.source),
                    )),
                    Some(old) if old.commit != entry.commit => events.push(Event::Moved(
                        plugin.name.clone(),
                        old.commit[..7.min(old.commit.len())].to_owned(),
                        short.to_owned(),
                    )),
                    Some(_) => {}
                }
                entries.insert(plugin.name.clone(), entry);
            }
            Err(err) => {
                events.push(Event::Error(plugin.name.clone(), format!("{err:#}")));
                if let Some(entry) = previous {
                    entries.insert(plugin.name.clone(), entry.clone());
                }
            }
        }
    }
    Ok((entries, events))
}

fn ensure_unpacked(
    paths: &Paths,
    git: &Git,
    plugin: &Plugin,
    commit: &str,
    dir: &str,
) -> Result<()> {
    let dest = store_dir(paths, &plugin.url, commit, dir);
    if dest.is_dir() {
        return Ok(());
    }
    let repo = git.ensure_commit(&plugin.url, commit)?;
    crate::sync::unpack_tree(git, &repo, commit, dir, &dest, |_| Ok(()))
}

/// One harness install skl owns.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Installed {
    pub agent: String,
    /// The manifest key.
    pub name: String,
    /// The harness's id for it, such as `pstack@skl`.
    pub id: String,
    /// The store folder the marketplace links to.
    pub target: PathBuf,
}

#[derive(Default, Serialize, Deserialize)]
pub struct State {
    #[serde(default)]
    pub installed: Vec<Installed>,
}

impl State {
    pub fn path(paths: &Paths) -> PathBuf {
        paths.data.join("plugins/state.toml")
    }

    pub fn load(paths: &Paths) -> Result<Self> {
        let path = Self::path(paths);
        match fs::read_to_string(&path) {
            Ok(text) => {
                toml::from_str(&text).with_context(|| format!("invalid {}", path.display()))
            }
            Err(err) if err.kind() == ErrorKind::NotFound => Ok(Self::default()),
            Err(err) => Err(err.into()),
        }
    }

    fn save(&self, paths: &Paths) -> Result<()> {
        crate::write_atomic(&Self::path(paths), toml::to_string(self)?.as_bytes())
    }

    pub fn get(&self, agent: &str, name: &str) -> Option<&Installed> {
        self.installed
            .iter()
            .find(|i| i.agent == agent && i.name == name)
    }

    fn set(&mut self, record: Installed) {
        self.installed
            .retain(|i| !(i.agent == record.agent && i.name == record.name));
        self.installed.push(record);
        self.installed
            .sort_by(|a, b| (&a.agent, &a.name).cmp(&(&b.agent, &b.name)));
    }

    fn drop(&mut self, agent: &str, name: &str) {
        self.installed
            .retain(|i| !(i.agent == agent && i.name == name));
    }
}

/// The marketplace skl generates for one harness.
pub fn marketplace_dir(paths: &Paths, harness: Harness) -> PathBuf {
    paths.data.join("plugins").join(harness.id())
}

/// The name a plugin's own manifest gives it, which Codex requires the marketplace
/// entry to match.
fn own_name(harness: Harness, dir: &Path) -> Option<String> {
    harness.manifests().iter().find_map(|m| {
        let text = fs::read_to_string(dir.join(m)).ok()?;
        let value: serde_json::Value = serde_json::from_str(&text).ok()?;
        Some(value.get("name")?.as_str()?.to_owned())
    })
}

/// What the sync report needs from installing plugins.
pub struct Outcome {
    pub notes: Vec<String>,
    pub errors: Vec<(String, String)>,
    pub conflicts: Vec<String>,
    /// Installs, updates and uninstalls made in harnesses.
    pub changed: usize,
}

/// Installs, updates and uninstalls plugins in each harness to match the lock.
/// Only installs recorded in plugin state are updated or removed. A harness that
/// isn't installed keeps its installs untouched.
pub fn apply(
    paths: &Paths,
    plugins: &[Plugin],
    locked: &BTreeMap<String, LockedPlugin>,
    failed: &BTreeSet<String>,
    active: &crate::profile::Active,
    dry_run: bool,
) -> Result<Outcome> {
    let mut outcome = Outcome {
        notes: Vec::new(),
        errors: Vec::new(),
        conflicts: Vec::new(),
        changed: 0,
    };
    let mut state = State::load(paths)?;
    let mut reload = Vec::new();
    for harness in Harness::ALL {
        let desired = desired(paths, plugins, locked, active, harness, |name| {
            failed.contains(name) && state.get(harness.id(), name).is_none()
        });
        let recorded: Vec<Installed> = state
            .installed
            .iter()
            .filter(|i| i.agent == harness.id())
            .cloned()
            .collect();
        if desired.is_empty() && recorded.is_empty() {
            continue;
        }
        let mut owners: BTreeMap<&str, &str> = BTreeMap::new();
        for (name, (own, _)) in &desired {
            if let Some(other) = owners.insert(own, name) {
                bail!(
                    "plugins `{other}` and `{name}` are both named `{own}` for {}; keep one",
                    harness.id()
                );
            }
        }
        if dry_run {
            for (name, (own, target)) in &desired {
                match recorded.iter().find(|i| &i.name == name) {
                    None => outcome.notes.push(format!(
                        "would install {own}@{MARKETPLACE} in {}",
                        harness.id()
                    )),
                    Some(record) if &record.target != target => outcome.notes.push(format!(
                        "would update {own}@{MARKETPLACE} in {}",
                        harness.id()
                    )),
                    Some(_) => continue,
                }
                outcome.changed += 1;
            }
            for record in recorded.iter().filter(|r| !desired.contains_key(&r.name)) {
                outcome.notes.push(format!(
                    "would uninstall {} from {}",
                    record.id,
                    harness.id()
                ));
                outcome.changed += 1;
            }
            continue;
        }
        if !harness.available()? {
            outcome.notes.push(format!(
                "{}: {}, so its plugins are skipped",
                harness.id(),
                harness.absence()
            ));
            continue;
        }
        let before = outcome.changed;
        apply_cli(
            paths,
            harness,
            &mut state,
            &recorded,
            &desired,
            &mut outcome,
        )?;
        if outcome.changed > before {
            reload.push(harness.reload());
        }
    }
    if let Some((last, rest)) = reload.split_last() {
        let steps = if rest.is_empty() {
            last.to_string()
        } else {
            format!("{}, and {last}", rest.join(", "))
        };
        outcome
            .notes
            .push(format!("{steps} to load plugin changes"));
    }
    Ok(outcome)
}

/// The plugins a harness should have, by manifest key: the plugin's own name and
/// the store folder to install. `skip` drops plugins that failed to resolve.
fn desired(
    paths: &Paths,
    plugins: &[Plugin],
    locked: &BTreeMap<String, LockedPlugin>,
    active: &crate::profile::Active,
    harness: Harness,
    skip: impl Fn(&str) -> bool,
) -> BTreeMap<String, (String, PathBuf)> {
    let mut desired = BTreeMap::new();
    for plugin in plugins
        .iter()
        .filter(|p| p.enabled(active) && p.agents.contains(&harness))
    {
        let Some(entry) = locked.get(&plugin.name) else {
            continue;
        };
        let Some(dir) = entry.subdirs.get(harness.id()) else {
            continue;
        };
        if skip(&plugin.name) {
            continue;
        }
        let target = store_dir(paths, &plugin.url, &entry.commit, dir);
        let own = own_name(harness, &target).unwrap_or_else(|| entry.plugin.clone());
        desired.insert(plugin.name.clone(), (own, target));
    }
    desired
}

/// Installs through Claude Code's or Codex's plugin CLI from skl's marketplace.
fn apply_cli(
    paths: &Paths,
    harness: Harness,
    state: &mut State,
    recorded: &[Installed],
    desired: &BTreeMap<String, (String, PathBuf)>,
    outcome: &mut Outcome,
) -> Result<()> {
    let program = harness.program();
    let installed = match run(program, &harness.list()).and_then(|out| harness.installed(&out)) {
        Ok(installed) => installed,
        Err(err) => {
            outcome
                .errors
                .push((harness.id().into(), format!("listing plugins: {err:#}")));
            return Ok(());
        }
    };
    // Uninstall first, while the marketplace still lists what's going away.
    for record in recorded.iter().filter(|r| !desired.contains_key(&r.name)) {
        if installed.contains(&record.id)
            && let Err(err) = run(program, &harness.uninstall(&record.id))
        {
            outcome.errors.push((
                record.name.clone(),
                format!("{} uninstall: {err:#}", harness.id()),
            ));
            continue;
        }
        state.drop(harness.id(), &record.name);
        state.save(paths)?;
        outcome
            .notes
            .push(format!("uninstalled {} from {}", record.id, harness.id()));
        outcome.changed += 1;
    }
    let marketplace = marketplace_dir(paths, harness);
    write_marketplace(&marketplace, desired)?;
    if desired.is_empty() {
        return Ok(());
    }
    if let Err(err) = register(harness, program, &marketplace) {
        outcome
            .errors
            .push((harness.id().into(), format!("{err:#}")));
        return Ok(());
    }
    for (name, (own, target)) in desired {
        let id = format!("{own}@{MARKETPLACE}");
        // The same plugin from another marketplace would load twice.
        if let Some(other) = installed.iter().find(|i| {
            i.split_once('@')
                .is_some_and(|(n, m)| n == own && m != MARKETPLACE)
        }) {
            outcome.conflicts.push(format!(
                "{} already has {other} installed outside skl: uninstall it there, or remove [plugins.{name}]",
                harness.id()
            ));
            continue;
        }
        let previous = state.get(harness.id(), name).cloned();
        let present = installed.contains(&id);
        let moved = previous.as_ref().is_some_and(|p| &p.target != target);
        let renamed = previous.as_ref().filter(|p| p.id != id);
        // Record the install before making it, so an interrupted run still owns it.
        state.set(Installed {
            agent: harness.id().into(),
            name: name.clone(),
            id: id.clone(),
            target: target.clone(),
        });
        state.save(paths)?;
        if let Some(old) = renamed
            && installed.contains(&old.id)
            && let Err(err) = run(program, &harness.uninstall(&old.id))
        {
            outcome
                .errors
                .push((name.clone(), format!("{}: {err:#}", harness.id())));
        }
        let action = if !present || renamed.is_some() {
            Some("installed")
        } else if moved && harness.copies() {
            Some("updated")
        } else {
            None
        };
        if moved && !harness.copies() {
            outcome
                .notes
                .push(format!("updated {id} in {}", harness.id()));
            outcome.changed += 1;
        }
        if let Some(action) = action {
            match run(program, &harness.install(&id)) {
                Ok(_) => {
                    outcome
                        .notes
                        .push(format!("{action} {id} in {}", harness.id()));
                    outcome.changed += 1;
                }
                Err(err) => {
                    // Nothing changed in the harness, so neither does the record.
                    match &previous {
                        Some(record) => state.set(record.clone()),
                        None => state.drop(harness.id(), name),
                    }
                    state.save(paths)?;
                    outcome
                        .errors
                        .push((name.clone(), format!("{} install: {err:#}", harness.id())));
                }
            }
        }
    }
    Ok(())
}

/// Writes the generated marketplace: one symlink per plugin into the store and a
/// marketplace file listing them. Links are replaced atomically.
fn write_marketplace(dir: &Path, desired: &BTreeMap<String, (String, PathBuf)>) -> Result<()> {
    fs::create_dir_all(dir.join(".claude-plugin"))?;
    let mut entries = Vec::new();
    for (own, target) in desired.values() {
        let link = dir.join(own);
        if fs::read_link(&link).ok().as_deref() != Some(target.as_path()) {
            let tmp = dir.join(format!(".{own}.tmp-{}", std::process::id()));
            let _ = fs::remove_file(&tmp);
            std::os::unix::fs::symlink(target, &tmp)
                .with_context(|| format!("linking {}", link.display()))?;
            fs::rename(&tmp, &link).with_context(|| format!("linking {}", link.display()))?;
        }
        entries.push(serde_json::json!({ "name": own, "source": format!("./{own}") }));
    }
    let wanted: BTreeSet<&str> = desired.values().map(|(own, _)| own.as_str()).collect();
    for entry in fs::read_dir(dir)? {
        let path = entry?.path();
        let name = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        if path.is_symlink() && !wanted.contains(name.as_str()) {
            fs::remove_file(&path)?;
        }
    }
    let marketplace = serde_json::json!({
        "name": MARKETPLACE,
        "owner": { "name": "skl" },
        "metadata": { "description": "Plugins pinned by skl. Edit skills.toml instead." },
        "plugins": entries,
    });
    crate::write_atomic(
        &dir.join(".claude-plugin/marketplace.json"),
        serde_json::to_string_pretty(&marketplace)?.as_bytes(),
    )
}

/// Registers skl's marketplace with a harness once. A different marketplace that
/// already uses the name is left alone.
fn register(harness: Harness, program: &str, dir: &Path) -> Result<()> {
    let listed = run(program, &harness.list_marketplaces())?;
    match harness.marketplace_path(&listed, MARKETPLACE)? {
        Some(path) if same_path(&path, dir) => Ok(()),
        Some(path) => bail!(
            "a marketplace named `{MARKETPLACE}` already points at {}: remove it from {} so skl can register its own",
            path.display(),
            harness.id()
        ),
        None => run(program, &harness.register(dir)).map(|_| ()),
    }
}

fn same_path(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    }
}

/// Runs a harness CLI with the user's environment, which locates its configuration.
fn run(program: &str, args: &[String]) -> Result<String> {
    const TIMEOUT: Duration = Duration::from_secs(300);
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("running {program}"))?;
    let mut stdout = child.stdout.take().expect("piped stdout");
    let mut stderr = child.stderr.take().expect("piped stderr");
    let out = std::thread::spawn(move || {
        let mut text = String::new();
        stdout.read_to_string(&mut text).map(|_| text)
    });
    let err = std::thread::spawn(move || {
        let mut text = String::new();
        stderr.read_to_string(&mut text).map(|_| text)
    });
    let start = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if start.elapsed() > TIMEOUT {
            let _ = child.kill();
            let _ = child.wait();
            bail!("`{program} {}` timed out", args.join(" "));
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    let stdout = out.join().expect("reader thread")?;
    let stderr = err.join().expect("reader thread")?;
    if !status.success() {
        let message = [stderr.trim(), stdout.trim()]
            .into_iter()
            .find(|s| !s.is_empty())
            .unwrap_or("no output");
        bail!("`{program} {}` failed: {message}", args.join(" "));
    }
    Ok(stdout)
}

/// The store folders plugins still use: locked commits and recorded installs.
pub fn live(
    paths: &Paths,
    locked: &BTreeMap<String, LockedPlugin>,
) -> Result<BTreeSet<(String, String)>> {
    let mut live = BTreeSet::new();
    for entry in locked.values() {
        let url = manifest::source_url(&entry.source)?;
        live.insert((crate::git::cache_key(&url), entry.commit.clone()));
    }
    let store = paths.store();
    for record in State::load(paths)?.installed {
        if let Ok(rest) = record.target.strip_prefix(&store) {
            let mut parts = rest
                .components()
                .map(|c| c.as_os_str().to_string_lossy().into_owned());
            if let (Some(key), Some(commit)) = (parts.next(), parts.next()) {
                live.insert((key, commit));
            }
        }
    }
    Ok(live)
}

/// What read-only commands show about one plugin, from the manifest, lock and
/// plugin state. Never runs an agent's CLI.
#[derive(Serialize)]
pub struct Report {
    pub name: String,
    /// Always `plugin`, beside a skill report's origin.
    pub origin: &'static str,
    pub manifest: PathBuf,
    pub source: String,
    pub git_ref: Option<String>,
    /// The plugin's name in the repository's marketplace.
    pub plugin: String,
    pub locked: Option<LockedPlugin>,
    pub lock_matches: bool,
    pub profiles: Vec<String>,
    pub active_profiles: Vec<String>,
    pub profile_source: String,
    pub os: Vec<String>,
    pub enabled: bool,
    pub exclusions: Vec<String>,
    pub agents: Vec<AgentReport>,
}

#[derive(Serialize)]
pub struct AgentReport {
    pub agent: String,
    /// Whether this machine has the agent.
    pub available: bool,
    /// The folder the manifest sets, or none when the marketplace finds it.
    pub subdir: Option<String>,
    pub locked_subdir: Option<String>,
    /// The plugin's store folder at the locked commit.
    pub content_path: Option<PathBuf>,
    pub content_available: bool,
    /// What skl recorded installing: the agent's id for it and its store folder.
    pub id: Option<String>,
    pub recorded_target: Option<PathBuf>,
    /// Where the agent loads it from: skl's marketplace link.
    pub install_path: Option<PathBuf>,
    pub status: InstallStatus,
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum InstallStatus {
    Installed,
    NotInstalled,
    /// Installed at an older commit or folder.
    Outdated,
    /// The agent isn't installed on this machine.
    Unavailable,
    /// Excluded by profiles or OS.
    Disabled,
    /// Recorded for an agent the plugin no longer lists.
    Stale,
}

impl InstallStatus {
    /// Whether the plugin needs a sync for this agent.
    pub fn needs_sync(self) -> bool {
        matches!(self, Self::NotInstalled | Self::Outdated | Self::Stale)
    }
}

impl std::fmt::Display for InstallStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Installed => "installed (owned)",
            Self::NotInstalled => "not installed",
            Self::Outdated => "installed from an older commit or folder (owned)",
            Self::Unavailable => "agent isn't installed",
            Self::Disabled => "excluded",
            Self::Stale => "installed but no longer selected (owned)",
        })
    }
}

/// Whether a store folder holds a plugin this harness reads.
fn readable(harness: Harness, dir: &Path) -> bool {
    harness.manifests().iter().any(|m| dir.join(m).is_file())
        || harness == Harness::ClaudeCode
            && ["skills", "agents", "commands"]
                .iter()
                .any(|folder| dir.join(folder).is_dir())
}

/// Where an agent loads a recorded install from.
pub fn install_path(paths: &Paths, harness: Harness, record: &Installed) -> PathBuf {
    marketplace_dir(paths, harness).join(record.id.split('@').next().unwrap_or_default())
}

pub fn report(
    paths: &Paths,
    active: &crate::profile::Active,
    plugin: &Plugin,
    lock: &crate::lock::Lock,
    state: &State,
) -> Result<Report> {
    let enabled = plugin.enabled(active);
    let mut exclusions = Vec::new();
    if !manifest::matches_os(&plugin.os) {
        exclusions.push(format!(
            "OS {} is outside [{}]",
            std::env::consts::OS,
            plugin.os.join(", ")
        ));
    }
    if !active.includes(&plugin.profiles) {
        exclusions.push(format!(
            "active profiles do not match [{}]",
            plugin.profiles.join(", ")
        ));
    }
    let locked = lock.plugins.get(&plugin.name).cloned();
    let lock_matches = locked.as_ref().is_some_and(|e| e.matches(plugin));
    let mut agents = Vec::new();
    for harness in Harness::ALL {
        let selected = plugin.agents.contains(&harness);
        let record = state.get(harness.id(), &plugin.name);
        if !selected && record.is_none() {
            continue;
        }
        let locked_subdir = locked
            .as_ref()
            .filter(|_| lock_matches)
            .and_then(|e| e.subdirs.get(harness.id()).cloned());
        let content_path = locked_subdir
            .as_ref()
            .map(|dir| store_dir(paths, &plugin.url, &locked.as_ref().unwrap().commit, dir));
        let content_available = content_path
            .as_deref()
            .is_some_and(|dir| readable(harness, dir));
        let available = harness.available()?;
        let install_path = record.map(|r| install_path(paths, harness, r));
        let status = if !selected || !enabled {
            if record.is_some() {
                InstallStatus::Stale
            } else {
                InstallStatus::Disabled
            }
        } else if !available {
            InstallStatus::Unavailable
        } else {
            match record {
                None => InstallStatus::NotInstalled,
                Some(r) if Some(&r.target) != content_path.as_ref() => InstallStatus::Outdated,
                Some(_) => InstallStatus::Installed,
            }
        };
        agents.push(AgentReport {
            agent: harness.id().into(),
            available,
            subdir: plugin.subdir.get(harness).map(str::to_owned),
            locked_subdir,
            content_path,
            content_available,
            id: record.map(|r| r.id.clone()),
            recorded_target: record.map(|r| r.target.clone()),
            install_path,
            status,
        });
    }
    Ok(Report {
        name: plugin.name.clone(),
        origin: "plugin",
        manifest: paths.manifest(),
        source: plugin.source.clone(),
        git_ref: plugin.git_ref.clone(),
        plugin: plugin.plugin.clone(),
        locked,
        lock_matches,
        profiles: plugin.profiles.clone(),
        active_profiles: active.names.clone(),
        profile_source: active.from.to_string(),
        os: plugin.os.clone(),
        enabled,
        exclusions,
        agents,
    })
}

impl Report {
    pub fn print(&self, paths: &Paths) {
        println!(
            "Plugin: {}\nManifest: {}\nSource: {}\nRef: {}",
            self.name,
            paths.tilde(&self.manifest),
            self.source,
            self.git_ref.as_deref().unwrap_or("default branch"),
        );
        if self.plugin != self.name {
            println!("Marketplace name: {}", self.plugin);
        }
        match &self.locked {
            Some(entry) => println!(
                "Pinned commit: {}\nLock: {}",
                entry.commit,
                if self.lock_matches {
                    "matches manifest"
                } else {
                    "stale (does not match manifest)"
                }
            ),
            None => println!("Lock: not locked"),
        }
        println!(
            "Profiles: {}\nActive profiles: {} ({})\nOS: {} (current: {})\nSelection: {}",
            if self.profiles.is_empty() {
                "all".into()
            } else {
                self.profiles.join(", ")
            },
            if self.active_profiles.is_empty() {
                "none".into()
            } else {
                self.active_profiles.join(", ")
            },
            self.profile_source,
            if self.os.is_empty() {
                "all".into()
            } else {
                self.os.join(", ")
            },
            std::env::consts::OS,
            if self.enabled {
                "included".into()
            } else {
                format!("excluded: {}", self.exclusions.join("; "))
            }
        );
        for agent in &self.agents {
            let status = match agent.status {
                InstallStatus::Unavailable => Harness::named(&agent.agent)
                    .map_or_else(|| agent.status.to_string(), Harness::absence),
                status => status.to_string(),
            };
            match &agent.id {
                Some(id) => println!("Agent {}: {status} as {id}", agent.agent),
                None => println!("Agent {}: {status}", agent.agent),
            }
            let folder = match (&agent.locked_subdir, &agent.subdir) {
                (Some(dir), _) | (None, Some(dir)) if dir.is_empty() => ".".to_owned(),
                (Some(dir), _) | (None, Some(dir)) => dir.clone(),
                (None, None) => "from the marketplace".into(),
            };
            println!("  Folder: {folder}");
            if let Some(path) = &agent.content_path {
                println!(
                    "  Content: {} ({})",
                    paths.tilde(path),
                    if agent.content_available {
                        "available"
                    } else {
                        "missing"
                    }
                );
            }
            if let Some(path) = &agent.install_path {
                println!("  Installed from: {}", paths.tilde(path));
            }
            if let Some(target) = agent
                .recorded_target
                .as_ref()
                .filter(|t| Some(*t) != agent.content_path.as_ref())
            {
                println!("  Recorded target: {}", paths.tilde(target));
            }
        }
    }
}
