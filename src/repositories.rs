//! Named repository declarations and persistent wildcard discovery.
use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use toml::{Table, Value};

use crate::agents::Agent;
use crate::git::Git;
use crate::manifest::{self, Manifest, Origin, Skill};
use crate::paths::Paths;

pub struct Repository {
    defaults: Table,
    overrides: BTreeSet<String>,
    excluded: BTreeSet<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Coverage {
    pub source: String,
    #[serde(rename = "ref", default, skip_serializing_if = "Option::is_none")]
    pub git_ref: Option<String>,
    /// Complete name-to-directory snapshot from the last successful discovery.
    pub members: BTreeMap<String, String>,
}

impl Repository {
    pub fn skill(&self, name: &str, agents: &[Agent], paths: &Paths) -> Result<Skill> {
        manifest::parse_skill(name, Value::Table(self.defaults.clone()), agents, paths)
    }
}

/// Expand selected lists and alias references before normal skill validation.
/// Wildcard groups are expanded later from discovery or lock coverage.
pub fn parse(
    table: &mut Table,
    agents: &[Agent],
    paths: &Paths,
) -> Result<BTreeMap<String, Repository>> {
    let declarations = match table.remove("repositories") {
        None => return Ok(BTreeMap::new()),
        Some(Value::Table(value)) => value,
        Some(_) => bail!("`repositories` must be a table"),
    };
    let mut skills = match table.remove("skills") {
        None => Table::new(),
        Some(Value::Table(value)) => value,
        Some(_) => bail!("`skills` must be a table"),
    };
    let mut groups = BTreeMap::new();
    let mut selections = BTreeMap::new();
    for (alias, value) in declarations {
        manifest::validate_name(&alias)?;
        let Value::Table(mut defaults) = value else {
            bail!("repository `{alias}` must be a table")
        };
        let selection = defaults.remove("skills");
        let exclusions = defaults.remove("exclude");
        if exclusions.is_some() && selection != Some(Value::String("*".into())) {
            bail!("repository `{alias}` exclude requires skills = \"*\"");
        }
        let excluded = match exclusions {
            None => BTreeSet::new(),
            Some(Value::Array(values)) => {
                let mut names = BTreeSet::new();
                for value in values {
                    let Value::String(name) = value else {
                        bail!("repository `{alias}` exclude must contain names")
                    };
                    manifest::validate_name(&name)?;
                    names.insert(name);
                }
                names
            }
            Some(_) => bail!("repository `{alias}` exclude must be an array of names"),
        };
        if defaults
            .keys()
            .any(|k| !matches!(k.as_str(), "source" | "ref" | "agents" | "profiles" | "os"))
        {
            bail!(
                "repository `{alias}` only supports source, ref, skills, exclude, agents, profiles, and os"
            );
        }
        let skill = manifest::parse_skill(&alias, Value::Table(defaults.clone()), agents, paths)?;
        if !matches!(skill.origin, Origin::Git(_)) {
            bail!("repository `{alias}` requires source")
        }
        let selection = match selection {
            None => Some(Vec::new()),
            Some(Value::String(value)) if value == "*" => None,
            Some(Value::Array(values)) => {
                let mut names = Vec::new();
                for value in values {
                    let Value::String(name) = value else {
                        bail!("repository `{alias}` skills must contain names")
                    };
                    manifest::validate_name(&name)?;
                    if !names.contains(&name) {
                        names.push(name);
                    }
                }
                Some(names)
            }
            _ => bail!("repository `{alias}` skills must be an array or \"*\""),
        };
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
    for (name, value) in &mut skills {
        let Value::Table(options) = value else {
            continue;
        };
        let Some(alias) = options.remove("repository") else {
            continue;
        };
        let Value::String(alias) = alias else {
            bail!("skill `{name}` repository must be an alias")
        };
        let group = groups
            .get_mut(&alias)
            .with_context(|| format!("skill `{name}` uses unknown repository `{alias}`"))?;
        if options.contains_key("source") || options.contains_key("path") {
            bail!("skill `{name}` cannot combine repository with source or path")
        }
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
                        "repository `{alias}` skill `{name}` conflicts with another declaration; use repository = \"{alias}\" for an override"
                    );
                }
            }
        }
    }
    table.insert("skills".into(), Value::Table(skills));
    groups.retain(|alias, _| selections[alias].is_none());
    Ok(groups)
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
                    "repository `{alias}` skill `{name}` conflicts with another declaration; use repository = \"{alias}\" for an override"
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
    let manifest = manifest::parse(&doc.to_string(), paths)?;
    for name in names {
        manifest::validate_name(name)?;
        let direct = doc.get("skills").and_then(|v| v.get(name));
        let override_alias = direct
            .and_then(|v| v.get("repository"))
            .and_then(toml_edit::Item::as_str);
        let aliases: Vec<_> = manifest
            .repositories
            .iter()
            .filter_map(|(alias, group)| {
                let member = coverage
                    .get(alias)
                    .is_some_and(|c| c.members.contains_key(name));
                ((member && !group.excluded.contains(name)) || override_alias == Some(alias))
                    .then(|| alias.clone())
            })
            .collect();
        if direct.is_none() && aliases.is_empty() {
            bail!("`{name}` isn't in the manifest; wildcard members require a previous `skl sync`");
        }
        if let Some(skills) = doc
            .get_mut("skills")
            .and_then(toml_edit::Item::as_table_like_mut)
        {
            skills.remove(name);
        }
        for alias in aliases {
            let group = &mut doc["repositories"][&alias];
            if group.get("exclude").is_none() {
                group["exclude"] = toml_edit::value(toml_edit::Array::new());
            }
            let excluded = group["exclude"]
                .as_array_mut()
                .context("exclude must be an array")?;
            if !excluded.iter().any(|value| value.as_str() == Some(name)) {
                excluded.push(name.as_str());
            }
        }
    }
    manifest::parse(&doc.to_string(), paths)?;
    Ok(())
}
