//! Declared sources (`[sources.<alias>]`) and persistent wildcard discovery.
use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use toml::{Table, Value};

use crate::agents::Agent;
use crate::export::{App, Preset, Targets};
use crate::git::Git;
use crate::manifest::{self, Manifest, Origin, Skill};
use crate::paths::Paths;

/// The tables that declare sources. `[repositories]` is the name 0.2.0 published.
pub const TABLES: [&str; 2] = ["sources", "repositories"];

pub struct Repository {
    defaults: Table,
    overrides: BTreeSet<String>,
    excluded: BTreeSet<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Coverage {
    /// The Git source, or empty for an app source.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub source: String,
    #[serde(rename = "ref", default, skip_serializing_if = "Option::is_none")]
    pub git_ref: Option<String>,
    /// The command whose export an app source's coverage came from.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub producer: Option<crate::manifest::CommandOrigin>,
    /// Complete name-to-directory snapshot from the last successful discovery.
    pub members: BTreeMap<String, String>,
}

impl Repository {
    pub fn skill(&self, name: &str, agents: &[Agent], paths: &Paths) -> Result<Skill> {
        manifest::parse_skill(name, Value::Table(self.defaults.clone()), agents, paths)
    }
}

/// A source declaration as written, for commands that edit the manifest.
pub struct Declaration {
    pub alias: String,
    /// The Git source, or None for an app source.
    pub source: Option<String>,
    pub git_ref: Option<String>,
    /// The export command of an app source.
    pub producer: Option<crate::manifest::CommandOrigin>,
    pub selection: Selection,
    /// The `agents`, `os` and `profiles` defaults as written.
    pub defaults: BTreeMap<String, BTreeSet<String>>,
}

pub enum Selection {
    /// No `skills` key: skills join through `source = "alias"` entries.
    Alias,
    List(Vec<String>),
    Wildcard {
        exclude: Vec<String>,
    },
}

impl Declaration {
    /// Whether the manifest selects `name` through this declaration's own list.
    /// Wildcard membership counts only when cached for this source and ref.
    pub fn selects(&self, name: &str, coverage: &BTreeMap<String, Coverage>) -> bool {
        match &self.selection {
            Selection::Alias => false,
            Selection::List(names) => names.iter().any(|n| n == name),
            Selection::Wildcard { exclude } => {
                !exclude.iter().any(|n| n == name) && self.covers(name, coverage)
            }
        }
    }

    /// Whether this declaration's current coverage lists `name`.
    pub fn covers(&self, name: &str, coverage: &BTreeMap<String, Coverage>) -> bool {
        coverage.get(&self.alias).is_some_and(|c| {
            let current = match &self.producer {
                Some(producer) => c.source.is_empty() && c.producer.as_ref() == Some(producer),
                None => self.source.as_ref() == Some(&c.source) && c.git_ref == self.git_ref,
            };
            current && c.members.contains_key(name)
        })
    }
}

/// A declared source's table, including the `[xcode]` shorthand.
pub fn declaration_mut<'a>(
    doc: &'a mut toml_edit::DocumentMut,
    alias: &str,
) -> Option<&'a mut dyn toml_edit::TableLike> {
    if alias == crate::export::XCODE && doc.contains_key("xcode") {
        return doc.get_mut("xcode")?.as_table_like_mut();
    }
    let table = TABLES
        .into_iter()
        .find(|table| doc.get(table).and_then(|t| t.get(alias)).is_some())?;
    doc.get_mut(table)?
        .as_table_like_mut()?
        .get_mut(alias)?
        .as_table_like_mut()
}

/// The declared source a `[skills]` entry refers to: `source = "alias"`, the string
/// form `"alias"`, or the older `repository = "alias"`. Git locations are never
/// bare names, so a name can only mean an alias.
pub fn reference(entry: &toml_edit::Item) -> Option<&str> {
    fn alias(item: Option<&toml_edit::Item>) -> Option<&str> {
        item.and_then(toml_edit::Item::as_str)
            .filter(|s| manifest::validate_name(s).is_ok())
    }
    match entry.as_table_like() {
        Some(table) => alias(table.get("repository")).or_else(|| alias(table.get("source"))),
        None => alias(Some(entry)),
    }
}

/// The export command an app source declares, as the manifest would parse it.
fn producer(group: &dyn toml_edit::TableLike) -> Option<crate::manifest::CommandOrigin> {
    let timeout = group
        .get("timeout")
        .and_then(toml_edit::Item::as_integer)
        .and_then(|n| u64::try_from(n).ok());
    if let Some(app) = group.get("app").and_then(toml_edit::Item::as_str) {
        return Some(Preset::named(app).ok()?.producer(timeout));
    }
    let argv = |key: &str| -> Option<Vec<String>> {
        let array = group.get(key)?.as_array()?;
        array
            .iter()
            .map(|v| v.as_str().map(str::to_owned))
            .collect()
    };
    Some(crate::manifest::CommandOrigin {
        command: argv("command")?,
        version_command: argv("version-command"),
        timeout: timeout.unwrap_or(30),
    })
}

/// Source declarations in manifest order. Malformed ones are skipped; manifest
/// validation reports them.
pub fn declarations(doc: &toml_edit::DocumentMut) -> Vec<Declaration> {
    let strings = |item: &toml_edit::Item| -> Option<Vec<String>> {
        let array = item.as_array()?;
        Some(
            array
                .iter()
                .filter_map(|v| v.as_str().map(str::to_owned))
                .collect(),
        )
    };
    let shorthand = doc
        .get("xcode")
        .and_then(toml_edit::Item::as_table_like)
        .map(|table| (crate::export::XCODE, table, true));
    let declared = TABLES
        .into_iter()
        .filter_map(|table| doc.get(table)?.as_table_like())
        .flat_map(|table| table.iter())
        .filter_map(|(alias, item)| Some((alias, item.as_table_like()?, false)));
    declared
        .chain(shorthand)
        .filter_map(|(alias, group, xcode)| {
            let selection = match group.get("skills") {
                None => Selection::Alias,
                Some(skills) if skills.as_str() == Some("*") => Selection::Wildcard {
                    exclude: group.get("exclude").and_then(strings).unwrap_or_default(),
                },
                Some(skills) => Selection::List(strings(skills)?),
            };
            let defaults = ["agents", "os", "profiles"]
                .into_iter()
                .filter_map(|key| {
                    let values = strings(group.get(key)?)?;
                    Some((key.to_owned(), values.into_iter().collect()))
                })
                .collect();
            let source = group
                .get("source")
                .and_then(toml_edit::Item::as_str)
                .map(str::to_owned);
            let producer = if xcode {
                Some(Preset::Xcode.producer(None))
            } else if source.is_none() {
                Some(producer(group)?)
            } else {
                None
            };
            Some(Declaration {
                alias: alias.to_owned(),
                source,
                git_ref: group
                    .get("ref")
                    .and_then(toml_edit::Item::as_str)
                    .map(str::to_owned),
                producer,
                selection,
                defaults,
            })
        })
        .collect()
}

/// Whether two manifest sources name one repository. GitHub shorthand, HTTPS and
/// SSH addresses are equivalent, ignoring case and a `.git` suffix. Elsewhere
/// `repo` and `repo.git` can be different repositories, so sources must match.
pub fn same_repository(a: &str, b: &str) -> bool {
    let key = |source: &str| {
        let url = manifest::source_url(source).ok()?;
        let url = url.trim_end_matches('/');
        for prefix in [
            "https://github.com/",
            "http://github.com/",
            "ssh://git@github.com/",
            "git@github.com:",
        ] {
            if let Some(path) = url.strip_prefix(prefix) {
                let path = path.strip_suffix(".git").unwrap_or(path);
                return Some(format!("github.com/{}", path.to_ascii_lowercase()));
            }
        }
        Some(url.to_owned())
    };
    key(a).is_some_and(|a| Some(a) == key(b))
}

/// Keeps a member's name in one place: its source's list while its entry only
/// names the source, otherwise its own `[skills]` entry. A wildcard that already
/// selects the name needs no entry, and any other bare reference takes the string
/// form, `name = "alias"`.
pub fn settle(doc: &mut toml_edit::DocumentMut, name: &str, coverage: &BTreeMap<String, Coverage>) {
    let Some(entry) = doc.get("skills").and_then(|skills| skills.get(name)) else {
        return;
    };
    let Some(alias) = reference(entry).map(str::to_owned) else {
        return;
    };
    let reference_only = entry.is_str() || entry.as_table_like().is_some_and(|t| t.len() == 1);
    let selected = declarations(doc).iter().any(|d| {
        d.alias == alias
            && matches!(&d.selection, Selection::Wildcard { exclude } if !exclude.iter().any(|n| n == name))
            && d.covers(name, coverage)
    });
    if reference_only && selected {
        if let Some(skills) = doc
            .get_mut("skills")
            .and_then(toml_edit::Item::as_table_like_mut)
        {
            skills.remove(name);
        }
        return;
    }
    let Some(list) = declaration_mut(doc, &alias)
        .and_then(|group| group.get_mut("skills"))
        .and_then(toml_edit::Item::as_array_mut)
    else {
        if reference_only
            && let Some(entry) = doc
                .get_mut("skills")
                .and_then(|skills| skills.get_mut(name))
            && let Some(value) = entry.as_value_mut()
            && !value.is_str()
        {
            let decor = value.decor().clone();
            *value = alias.as_str().into();
            *value.decor_mut() = decor;
        }
        return;
    };
    if !reference_only {
        remove_name(list, name);
        return;
    }
    if !list.iter().any(|v| v.as_str() == Some(name)) {
        insert_name(list, name);
    }
    if let Some(skills) = doc
        .get_mut("skills")
        .and_then(toml_edit::Item::as_table_like_mut)
    {
        skills.remove(name);
    }
}

/// The whitespace and comments before element `index`, or before `]` past the end.
fn gap(array: &toml_edit::Array, index: usize) -> String {
    match array.get(index) {
        Some(value) => value.decor().prefix().and_then(|p| p.as_str()),
        None => array.trailing().as_str(),
    }
    .unwrap_or_default()
    .to_owned()
}

fn set_gap(array: &mut toml_edit::Array, index: usize, gap: String) {
    match array.get_mut(index) {
        Some(value) => value.decor_mut().set_prefix(gap),
        None => array.set_trailing(gap),
    }
}

/// Inserts a name the way `cargo add` edits a dependency table: in order when the
/// list is already sorted, otherwise last, laid out like its neighbours.
pub fn insert_name(array: &mut toml_edit::Array, name: &str) {
    let names: Vec<_> = array.iter().filter_map(|v| v.as_str()).collect();
    let sorted = names.len() == array.len() && names.is_sorted();
    let index = if sorted {
        names.partition_point(|n| *n < name)
    } else {
        array.len()
    };
    let mut value = toml_edit::Value::from(name);
    let first = gap(array, 0);
    if let Some(at) = first.rfind('\n') {
        // One name per line. The new line takes the rest of the line before it, such
        // as a trailing comment, and the next name keeps the comments above it.
        let indent = if array.is_empty() {
            "  "
        } else {
            &first[at + 1..]
        };
        let next = gap(array, index);
        let prefix = match next.split_once('\n') {
            Some((line, above)) => {
                let prefix = format!("{line}\n{indent}");
                set_gap(array, index, format!("\n{above}"));
                prefix
            }
            None => format!("\n{indent}"),
        };
        value.decor_mut().set_prefix(prefix);
    } else if !array.is_empty() {
        if index == 0 {
            set_gap(array, 0, " ".into());
        }
        value
            .decor_mut()
            .set_prefix(if index == 0 { first } else { " ".into() });
    }
    value.decor_mut().set_suffix("");
    array.insert_formatted(index, value);
}

/// Removes every occurrence of a name with its own line and the comments above it.
pub fn remove_name(array: &mut toml_edit::Array, name: &str) -> bool {
    let mut removed = false;
    loop {
        let found = array.iter().position(|v| v.as_str() == Some(name));
        let Some(index) = found else { break };
        let gone = gap(array, index);
        array.remove(index);
        let next = gap(array, index);
        // The removed gap starts with the rest of the previous line, which stays.
        let kept = match (gone.split_once('\n'), next.split_once('\n')) {
            (Some((line, _)), Some((_, above))) => format!("{line}\n{above}"),
            _ if index == 0 => gone,
            _ => next,
        };
        set_gap(array, index, kept);
        removed = true;
    }
    if removed && array.is_empty() {
        array.set_trailing("");
        array.set_trailing_comma(false);
    }
    removed
}

/// Expand selected lists and alias references before normal skill validation.
/// Wildcard groups are expanded later from discovery or lock coverage.
pub fn parse(
    table: &mut Table,
    agents: &[Agent],
    paths: &Paths,
) -> Result<(BTreeMap<String, Repository>, BTreeMap<String, App>)> {
    let mut declarations = Table::new();
    for key in TABLES {
        match table.remove(key) {
            None => {}
            Some(Value::Table(value)) => {
                for (alias, group) in value {
                    if declarations.insert(alias.clone(), group).is_some() {
                        bail!("declare source `{alias}` once, in `[sources]`");
                    }
                }
            }
            Some(_) => bail!("`{key}` must be a table"),
        }
    }
    let mut skills = match table.remove("skills") {
        None => Table::new(),
        Some(Value::Table(value)) => value,
        Some(_) => bail!("`skills` must be a table"),
    };
    let mut groups = BTreeMap::new();
    let mut apps = BTreeMap::new();
    let mut selections = BTreeMap::new();
    for (alias, value) in declarations {
        manifest::validate_name(&alias)?;
        let Value::Table(mut defaults) = value else {
            bail!("source `{alias}` must be a table")
        };
        let selection = defaults.remove("skills");
        let exclusions = defaults.remove("exclude");
        if exclusions.is_some() && selection != Some(Value::String("*".into())) {
            bail!("source `{alias}` exclude requires skills = \"*\"");
        }
        let excluded = match exclusions {
            None => BTreeSet::new(),
            Some(Value::Array(values)) => {
                let mut names = BTreeSet::new();
                for value in values {
                    let Value::String(name) = value else {
                        bail!("source `{alias}` exclude must contain names")
                    };
                    manifest::validate_name(&name)?;
                    names.insert(name);
                }
                names
            }
            Some(_) => bail!("source `{alias}` exclude must be an array of names"),
        };
        let selection = match selection {
            None => Some(Vec::new()),
            Some(Value::String(value)) if value == "*" => None,
            Some(Value::Array(values)) => {
                let mut names = Vec::new();
                for value in values {
                    let Value::String(name) = value else {
                        bail!("source `{alias}` skills must contain names")
                    };
                    manifest::validate_name(&name)?;
                    if !names.contains(&name) {
                        names.push(name);
                    }
                }
                Some(names)
            }
            _ => bail!("source `{alias}` skills must be an array or \"*\""),
        };
        if defaults.contains_key("command") || defaults.contains_key("app") {
            let app = app_repository(&alias, defaults, selection, excluded, agents, paths)
                .with_context(|| format!("source `{alias}`"))?;
            apps.insert(alias, app);
            continue;
        }
        if !defaults.contains_key("source") {
            bail!("source `{alias}` requires source, command or app")
        }
        if defaults
            .keys()
            .any(|k| !matches!(k.as_str(), "source" | "ref" | "agents" | "profiles" | "os"))
        {
            bail!(
                "source `{alias}` only supports source, ref, skills, exclude, agents, profiles, and os"
            );
        }
        let skill = manifest::parse_skill(&alias, Value::Table(defaults.clone()), agents, paths)?;
        if !matches!(skill.origin, Origin::Git(_)) {
            bail!("source `{alias}` requires source, command or app")
        }
        selections.insert(alias.clone(), selection);
        groups.insert(
            alias,
            Repository {
                defaults,
                overrides: BTreeSet::new(),
                excluded,
            },
        );
    }
    let mut members = Vec::new();
    for (name, value) in &mut skills {
        let alias = match &*value {
            Value::String(alias) if manifest::validate_name(alias).is_ok() => alias.clone(),
            Value::Table(options) => match (options.get("repository"), options.get("source")) {
                (Some(Value::String(alias)), _) => alias.clone(),
                (Some(_), _) => bail!("skill `{name}` repository must be an alias"),
                (None, Some(Value::String(alias))) if manifest::validate_name(alias).is_ok() => {
                    alias.clone()
                }
                _ => continue,
            },
            _ => continue,
        };
        if value.is_str() {
            *value = Value::Table(Table::new());
        }
        let Value::Table(options) = value else {
            unreachable!()
        };
        if options.remove("repository").is_none() {
            options.remove("source");
        }
        if options.contains_key("source") || options.contains_key("path") {
            bail!("skill `{name}` refers to source `{alias}`, so it can't also set source or path")
        }
        if let Some(app) = apps.get_mut(&alias) {
            app.overrides.insert(
                name.clone(),
                member_targets(name, options, &app.defaults, agents, paths)?,
            );
            members.push(name.clone());
            continue;
        }
        let group = groups.get_mut(&alias).with_context(|| {
            format!(
                "skill `{name}` refers to unknown source `{alias}`: declare [sources.{alias}], or use owner/repo, a git URL, or an absolute path"
            )
        })?;
        let mut merged = group.defaults.clone();
        merged.extend(options.clone());
        *options = merged;
        group.overrides.insert(name.clone());
    }
    for (alias, selection) in &selections {
        if let Some(names) = selection {
            let group = &groups[alias];
            for name in names {
                if group.overrides.contains(name) {
                    continue;
                }
                if skills
                    .insert(name.clone(), Value::Table(group.defaults.clone()))
                    .is_some()
                {
                    bail!(
                        "source `{alias}` skill `{name}` conflicts with another declaration; use source = \"{alias}\" for an override"
                    );
                }
            }
        }
    }
    // App members are added once their app's export is known.
    for name in members {
        skills.remove(&name);
    }
    table.insert("skills".into(), Value::Table(skills));
    groups.retain(|alias, _| selections[alias].is_none());
    Ok((groups, apps))
}

/// An app source: `command` exports skills into `{output}`, or `app` names an
/// app skl knows, such as Xcode.
fn app_repository(
    alias: &str,
    mut defaults: Table,
    skills: Option<Vec<String>>,
    exclude: BTreeSet<String>,
    agents: &[Agent],
    paths: &Paths,
) -> Result<App> {
    if let Some(key) = defaults.keys().find(|k| {
        !matches!(
            k.as_str(),
            "command" | "version-command" | "timeout" | "app" | "agents" | "profiles" | "os"
        )
    }) {
        bail!(
            "unknown key `{key}`: app sources support app, command, version-command, timeout, skills, exclude, agents, profiles, and os"
        );
    }
    let preset = match defaults.remove("app") {
        None => None,
        Some(Value::String(name)) => Some(Preset::named(&name)?),
        Some(_) => bail!("`app` must be a name"),
    };
    let mut timeout = None;
    if let Some(preset) = preset {
        if defaults.contains_key("command") || defaults.contains_key("version-command") {
            bail!("set either `app` or `command`, not both");
        }
        timeout = match defaults.remove("timeout") {
            None => None,
            Some(Value::Integer(n)) if (1..=300).contains(&n) => Some(n as u64),
            Some(_) => bail!("timeout must be an integer from 1 to 300 seconds"),
        };
        defaults
            .entry("os")
            .or_insert_with(|| Value::Array(vec![Value::String(preset.os().into())]));
        defaults.insert("path".into(), Value::String(".".into()));
    }
    let template = manifest::parse_skill(alias, Value::Table(defaults), agents, paths)?;
    let producer = match (preset, template.origin) {
        (Some(preset), _) => preset.producer(timeout),
        (None, Origin::Command(origin)) => {
            if !origin.command.iter().any(|arg| arg == "{output}") {
                bail!("`command` must export skills into the `{{output}}` directory");
            }
            origin
        }
        (None, _) => unreachable!("an app source sets command"),
    };
    Ok(App {
        producer,
        preset,
        skills,
        exclude,
        defaults: Targets {
            agents: template.agents,
            profiles: template.profiles,
            os: template.os,
        },
        overrides: BTreeMap::new(),
    })
}

/// The settings an app member overrides, inheriting the rest from its source.
fn member_targets(
    name: &str,
    options: &Table,
    defaults: &Targets,
    agents: &[Agent],
    paths: &Paths,
) -> Result<Targets> {
    if let Some(key) = options
        .keys()
        .find(|k| !matches!(k.as_str(), "agents" | "profiles" | "os"))
    {
        bail!(
            "skill `{name}` comes from an app, so it can only override agents, profiles, and os, not `{key}`"
        );
    }
    let mut probe = options.clone();
    probe.insert("path".into(), Value::String(".".into()));
    let parsed = manifest::parse_skill(name, Value::Table(probe), agents, paths)?;
    let mut targets = defaults.clone();
    if options.contains_key("agents") {
        targets.agents = parsed.agents;
    }
    if options.contains_key("profiles") {
        targets.profiles = parsed.profiles;
    }
    if options.contains_key("os") {
        targets.os = parsed.os;
    }
    Ok(targets)
}

pub fn discover(
    git: &Git,
    group: &Repository,
    agents: &[Agent],
    paths: &Paths,
) -> Result<(Coverage, String)> {
    let skill = group.skill("repository", agents, paths)?;
    let Origin::Git(origin) = skill.origin else {
        unreachable!()
    };
    let commit = git.resolve(&origin.url, origin.git_ref.as_deref())?;
    let repo = git.ensure_commit(&origin.url, &commit)?;
    let mut members = BTreeMap::new();
    for candidate in git.candidates(&repo, &commit)? {
        manifest::validate_name(&candidate.name)?;
        if members
            .insert(candidate.name.clone(), candidate.subdir)
            .is_some()
        {
            bail!(
                "ambiguous repository skill `{}`; select explicit skills with subdir instead of wildcard",
                candidate.name
            );
        }
    }
    Ok((
        Coverage {
            source: origin.source,
            git_ref: origin.git_ref,
            producer: None,
            members,
        },
        commit,
    ))
}

pub fn matches(
    group: &Repository,
    coverage: &Coverage,
    agents: &[Agent],
    paths: &Paths,
) -> Result<bool> {
    let skill = group.skill("repository", agents, paths)?;
    let Origin::Git(origin) = skill.origin else {
        unreachable!()
    };
    Ok(origin.source == coverage.source && origin.git_ref == coverage.git_ref)
}

pub fn append(
    paths: &Paths,
    manifest: &mut Manifest,
    coverage: &BTreeMap<String, Coverage>,
) -> Result<()> {
    for (alias, group) in &manifest.repositories {
        let Some(coverage) = coverage.get(alias) else {
            continue;
        };
        for (name, subdir) in &coverage.members {
            if group.overrides.contains(name) || group.excluded.contains(name) {
                continue;
            }
            if manifest.skills.iter().any(|skill| skill.name == *name) {
                bail!(
                    "source `{alias}` skill `{name}` conflicts with another declaration; use source = \"{alias}\" for an override"
                );
            }
            let mut skill = group.skill(name, &manifest.agents, paths)?;
            let Origin::Git(origin) = &mut skill.origin else {
                unreachable!()
            };
            origin.subdir = Some(subdir.clone());
            manifest.skills.push(skill);
        }
    }
    Ok(())
}

pub fn pending(
    paths: &Paths,
    manifest: &Manifest,
    coverage: &BTreeMap<String, Coverage>,
    active: &crate::profile::Active,
) -> Result<Vec<String>> {
    let mut pending = Vec::new();
    for (alias, group) in &manifest.repositories {
        if group.skill(alias, &manifest.agents, paths)?.enabled(active)
            && match coverage.get(alias) {
                None => true,
                Some(entry) => !matches(group, entry, &manifest.agents, paths)?,
            }
        {
            pending.push(alias.clone());
        }
    }
    for (alias, app) in &manifest.apps {
        if app.skills.is_none() && app.enabled(active) && app.coverage(alias, coverage).is_none() {
            pending.push(alias.clone());
        }
    }
    Ok(pending)
}

/// Remove wildcard-derived skills without allowing discovery to add them back.
/// Coverage stays complete; exclusions only filter the expanded manifest.
pub fn exclude_removed(
    paths: &Paths,
    doc: &mut toml_edit::DocumentMut,
    coverage: &BTreeMap<String, Coverage>,
    names: &[String],
) -> Result<()> {
    manifest::parse(&doc.to_string(), paths)?;
    for name in names {
        manifest::validate_name(name)?;
        let declared = declarations(doc);
        let listed: Vec<_> = declared
            .iter()
            .filter(|d| matches!(&d.selection, Selection::List(names) if names.contains(name)))
            .map(|d| d.alias.clone())
            .collect();
        let direct = doc.get("skills").and_then(|v| v.get(name));
        let override_alias = direct.and_then(reference);
        let aliases: Vec<_> = declared
            .iter()
            .filter_map(|d| {
                let Selection::Wildcard { exclude } = &d.selection else {
                    return None;
                };
                let member = d.covers(name, coverage) && !exclude.contains(name);
                (member || override_alias == Some(d.alias.as_str())).then(|| d.alias.clone())
            })
            .collect();
        if direct.is_none() && aliases.is_empty() && listed.is_empty() {
            bail!("`{name}` isn't in the manifest; wildcard members require a previous `skl sync`");
        }
        if let Some(skills) = doc
            .get_mut("skills")
            .and_then(toml_edit::Item::as_table_like_mut)
        {
            skills.remove(name);
        }
        for alias in listed {
            if let Some(list) = declaration_mut(doc, &alias)
                .and_then(|group| group.get_mut("skills"))
                .and_then(toml_edit::Item::as_array_mut)
            {
                remove_name(list, name);
            }
        }
        for alias in aliases {
            let group =
                declaration_mut(doc, &alias).context("source declarations must be tables")?;
            if group.get("exclude").is_none() {
                group.insert("exclude", toml_edit::value(toml_edit::Array::new()));
            }
            let excluded = group
                .get_mut("exclude")
                .and_then(toml_edit::Item::as_array_mut)
                .context("exclude must be an array")?;
            if !excluded.iter().any(|value| value.as_str() == Some(name)) {
                excluded.push(name.as_str());
            }
        }
    }
    manifest::parse(&doc.to_string(), paths)?;
    Ok(())
}
