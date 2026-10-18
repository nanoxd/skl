//! Saved plans are review artifacts, not signatures. Treat them as trusted input.
use std::collections::BTreeMap;
use std::fs;
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result, bail};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{
    git::Git,
    lock::Lock,
    manifest::{self, Origin},
    paths::Paths,
    profile::Active,
};

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Context {
    manifest: Option<String>,
    lock: Option<String>,
    home: PathBuf,
    config: PathBuf,
    data: PathBuf,
    profiles: Vec<String>,
    os: String,
    destinations: BTreeMap<String, Vec<PathBuf>>,
    local_paths: BTreeMap<String, PathBuf>,
}

fn read_optional(path: &Path) -> Result<Option<String>> {
    match fs::read_to_string(path) {
        Ok(text) => Ok(Some(text)),
        Err(err) if err.kind() == ErrorKind::NotFound => Ok(None),
        Err(err) => Err(err).with_context(|| format!("reading {}", path.display())),
    }
}

fn absolute(path: &Path) -> Result<PathBuf> {
    // Resolve existing ancestors too, including aliases to currently absent paths.
    if let Ok(real) = path.canonicalize() {
        return Ok(real);
    }
    let path = std::path::absolute(path)?;
    let parent = path.parent().context("path has no parent")?;
    Ok(absolute(parent)?.join(path.file_name().context("path has no filename")?))
}

impl Context {
    pub fn read(paths: &Paths, active: &Active) -> Result<Self> {
        let mut profiles = active.names.clone();
        profiles.sort();
        let parsed = manifest::load(paths)?;
        let mut destinations = BTreeMap::new();
        let mut local_paths = BTreeMap::new();
        let skills = parsed.skills;
        for (alias, group) in &parsed.repositories {
            let skill = group.skill(alias, &parsed.agents, paths)?;
            destinations.insert(
                format!("repository:{alias}"),
                skill
                    .agents
                    .iter()
                    .map(|a| absolute(&a.dir))
                    .collect::<Result<Vec<_>>>()?,
            );
        }
        if let Some(config) = &parsed.xcode {
            for agent in &config.agents {
                destinations.insert(
                    format!("xcode:{}", agent.dir.display()),
                    vec![absolute(&agent.dir)?],
                );
            }
        }
        for skill in skills {
            destinations.insert(
                format!("skill:{}", skill.name),
                skill
                    .agents
                    .iter()
                    .map(|a| absolute(&a.dir))
                    .collect::<Result<Vec<_>>>()?,
            );
            if let Origin::Local(dir) | Origin::OptionalLocal(dir) = skill.origin {
                local_paths.insert(skill.name, absolute(&dir)?);
            }
        }
        Ok(Self {
            destinations,
            local_paths,
            manifest: read_optional(&paths.manifest())?,
            lock: read_optional(&paths.lock())?,
            home: absolute(&paths.home)?,
            config: absolute(&paths.config)?,
            data: absolute(&paths.data)?,
            profiles,
            os: std::env::consts::OS.into(),
        })
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Plan {
    version: u32,
    context: Context,
    pub names: Vec<String>,
    pub target: Lock,
    /// Full patches are stored with the exact target commits for review.
    changes: BTreeMap<String, String>,
    #[serde(default)]
    pub xcode_snapshots: BTreeMap<String, crate::xcode::Snapshot>,
    checksum: String,
}

impl Plan {
    fn checksum(&self) -> Result<String> {
        #[derive(Serialize)]
        struct Payload<'a> {
            context: &'a Context,
            names: &'a [String],
            target: &'a Lock,
            changes: &'a BTreeMap<String, String>,
            xcode_snapshots: &'a BTreeMap<String, crate::xcode::Snapshot>,
        }
        let payload = Payload {
            context: &self.context,
            names: &self.names,
            target: &self.target,
            changes: &self.changes,
            xcode_snapshots: &self.xcode_snapshots,
        };
        Ok(format!(
            "{:x}",
            Sha256::digest(toml::to_string(&payload)?.as_bytes())
        ))
    }

    pub fn load(file: &Path, paths: &Paths, active: &Active) -> Result<Self> {
        let plan: Self = toml::from_str(
            &fs::read_to_string(file)
                .with_context(|| format!("reading plan {}", paths.tilde(file)))?,
        )
        .with_context(|| format!("invalid update plan {}", paths.tilde(file)))?;
        if plan.version != 2 {
            bail!("unsupported update plan version {}", plan.version);
        }
        if plan.context != Context::read(paths, active)? {
            bail!(
                "update plan is stale: manifest, lock, profiles, OS or machine paths changed; generate a new plan"
            );
        }
        if plan.checksum != plan.checksum()? {
            bail!("update plan checksum mismatch; generate a new plan instead of editing it");
        }
        let mut manifest = manifest::load(paths)?;
        let old = Lock::load(&paths.lock())?;
        for (alias, group) in &manifest.repositories {
            let coverage = plan.target.repositories.get(alias);
            let enabled = group.skill(alias, &manifest.agents, paths)?.enabled(active);
            if !enabled {
                if coverage != old.repositories.get(alias) {
                    bail!("plan changes inactive repository `{alias}`");
                }
            } else {
                let coverage = coverage.context("plan missing wildcard coverage")?;
                if !crate::repositories::matches(group, coverage, &manifest.agents, paths)? {
                    bail!("plan wildcard source mismatch");
                }
            }
        }
        if plan
            .target
            .repositories
            .keys()
            .any(|alias| !manifest.repositories.contains_key(alias))
        {
            bail!("unexpected repository in plan");
        }
        for coverage in plan.target.repositories.values() {
            for (name, subdir) in &coverage.members {
                manifest::validate_name(name)?;
                if subdir.starts_with('/')
                    || (!subdir.is_empty()
                        && subdir
                            .split('/')
                            .any(|part| matches!(part, "" | "." | "..")))
                {
                    bail!("invalid repository directory for `{name}` in update plan");
                }
            }
        }
        crate::repositories::append(paths, &mut manifest, &plan.target.repositories)?;
        if let Some(config) = &manifest.xcode {
            if !config.enabled(active)
                && (plan.target.xcode != old.xcode
                    || plan.target.xcode_all_complete != old.xcode_all_complete)
            {
                bail!("plan changes inactive Xcode imports");
            }
            for (name, entry) in &plan.target.xcode {
                manifest::validate_name(name)?;
                if config.enabled(active) && !config.selects(name) {
                    bail!("unexpected Xcode skill in plan");
                }
                if let Some(snapshot) = plan.xcode_snapshots.get(name) {
                    snapshot.verify(entry)?;
                } else if old.xcode.get(name) != Some(entry) {
                    bail!("plan missing Xcode snapshot for `{name}`");
                }
            }
        } else if !plan.target.xcode.is_empty() || plan.target.xcode_all_complete {
            bail!("unexpected Xcode metadata in plan");
        }
        if plan
            .xcode_snapshots
            .keys()
            .any(|name| !plan.target.xcode.contains_key(name))
        {
            bail!("unexpected Xcode snapshot in plan");
        }

        if plan.target.version != old.version {
            bail!("invalid update plan lock metadata");
        }
        let mut expected = BTreeMap::new();
        for skill in &manifest.skills {
            if !skill.enabled(active) {
                if let Some(entry) = old.skills.get(&skill.name) {
                    expected.insert(skill.name.clone(), entry.clone());
                }
                continue;
            }
            if let Origin::Git(origin) = &skill.origin {
                let entry = plan
                    .target
                    .skills
                    .get(&skill.name)
                    .with_context(|| format!("update plan is missing `{}`", skill.name))?;
                if !entry.matches(origin)
                    || ![40, 64].contains(&entry.commit.len())
                    || !entry.commit.bytes().all(|b| b.is_ascii_hexdigit())
                {
                    bail!("invalid update plan entry for `{}`", skill.name);
                }
                let selected = plan.names.is_empty() || plan.names.contains(&skill.name);
                if !selected
                    && old
                        .skills
                        .get(&skill.name)
                        .is_some_and(|e| e.matches(origin) && e != entry)
                {
                    bail!("update plan changes unselected skill `{}`", skill.name);
                }
                expected.insert(skill.name.clone(), entry.clone());
            }
        }
        if expected != plan.target.skills {
            bail!("update plan has unexpected or changed inactive skills");
        }
        Ok(plan)
    }

    pub fn save(
        file: &Path,
        paths: &Paths,
        active: &Active,
        context: Context,
        names: Vec<String>,
        artifact: (Lock, BTreeMap<String, crate::xcode::Snapshot>),
        git: &Git,
    ) -> Result<()> {
        let (target, xcode_snapshots) = artifact;
        if context != Context::read(paths, active)? {
            bail!("manifest or lock changed while planning; generate a new plan");
        }
        let manifest = manifest::load(paths)?;
        let output = absolute(file)?;
        let mut protected = vec![absolute(&paths.config)?, absolute(&paths.data)?];
        protected.extend(context.destinations.values().flatten().cloned());
        for skill in &manifest.skills {
            protected.extend(
                skill
                    .agents
                    .iter()
                    .map(|a| absolute(&a.dir))
                    .collect::<Result<Vec<_>>>()?,
            );
            if let Origin::Local(dir) | Origin::OptionalLocal(dir) = &skill.origin {
                protected.push(absolute(dir)?);
            }
        }
        if protected.iter().any(|dir| output.starts_with(dir)) {
            bail!("save update plans outside skl config, data, agent and local skill directories");
        }
        let old = Lock::load(&paths.lock())?;
        let mut changes = BTreeMap::new();
        for (name, entry) in &target.skills {
            if old.skills.get(name) == Some(entry) {
                continue;
            }
            let url = manifest::source_url(&entry.source)?;
            let repo = git.ensure_commit(&url, &entry.commit)?;
            let patch =
                if let Some(previous) = old.skills.get(name).filter(|p| p.source == entry.source) {
                    crate::sync::describe_change(git, previous, entry, &url, true)?
                } else {
                    let tree = if entry.subdir.is_empty() {
                        entry.commit.clone()
                    } else {
                        format!("{}:{}", entry.commit, entry.subdir)
                    };
                    git.diff(
                        &repo,
                        "4b825dc642cb6eb9a060e54bf8d69288fbee4904",
                        &tree,
                        true,
                    )?
                };
            changes.insert(name.clone(), patch);
        }
        for (name, snapshot) in &xcode_snapshots {
            snapshot.verify(&target.xcode[name])?;
            if let Some(entry) = old.xcode.get(name) {
                crate::xcode::Snapshot::capture(&crate::xcode::skill_dir(paths, name, entry))?
                    .verify(entry)?;
            }
            if old.xcode.get(name) != target.xcode.get(name) {
                let previous = old
                    .xcode
                    .get(name)
                    .map(|entry| crate::xcode::skill_dir(paths, name, entry));
                changes.insert(
                    format!("xcode:{name}"),
                    snapshot.review(match &previous {
                        Some(dir) if !dir.join("SKILL.md").is_file() => bail!("previous Xcode content for `{name}` is unavailable; cannot review its changes"),
                        value => value.as_deref(),
                    }, false)?,
                );
            }
        }
        for (name, entry) in &old.xcode {
            if !target.xcode.contains_key(name) {
                let previous = crate::xcode::skill_dir(paths, name, entry);
                let snapshot = crate::xcode::Snapshot::capture(&previous)?;
                snapshot.verify(entry)?;
                changes.insert(
                    format!("removed:xcode:{name}"),
                    snapshot.review(None, true)?,
                );
            }
        }
        for (name, entry) in &old.skills {
            if !target.skills.contains_key(name) {
                let url = manifest::source_url(&entry.source)?;
                let repo = git.ensure_commit(&url, &entry.commit)?;
                let tree = if entry.subdir.is_empty() {
                    entry.commit.clone()
                } else {
                    format!("{}:{}", entry.commit, entry.subdir)
                };
                changes.insert(
                    format!("removed:{name}"),
                    git.diff(
                        &repo,
                        &tree,
                        "4b825dc642cb6eb9a060e54bf8d69288fbee4904",
                        true,
                    )?,
                );
            }
        }
        let mut plan = Self {
            version: 2,
            context,
            names,
            target,
            xcode_snapshots,
            changes,
            checksum: String::new(),
        };
        plan.checksum = plan.checksum()?;
        let text = toml::to_string_pretty(&plan)?;
        let mut temporary =
            tempfile::NamedTempFile::new_in(output.parent().context("plan output has no parent")?)?;
        temporary.write_all(text.as_bytes())?;
        temporary.flush()?;
        temporary.persist_noclobber(&output).with_context(|| {
            format!(
                "saving plan {} (output must not already exist)",
                paths.tilde(file)
            )
        })?;
        Ok(())
    }
}
