//! Read-only inspection: collecting a report never resolves or exports a source.
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::Serialize;

use crate::{
    git,
    lock::Lock,
    manifest::{self, Origin},
    paths::Paths,
    profile::Active,
    sync, xcode,
};

#[derive(Serialize)]
pub struct Report {
    pub name: String,
    pub manifest: PathBuf,
    pub origin: String,
    pub source: Option<String>,
    pub git_ref: Option<String>,
    pub subdir: Option<String>,
    pub locked: Option<crate::lock::LockedSkill>,
    pub lock_matches: bool,
    pub xcode: Option<crate::lock::XcodeSkill>,
    pub content_path: Option<PathBuf>,
    pub content_available: bool,
    pub profiles: Vec<String>,
    pub active_profiles: Vec<String>,
    pub profile_source: String,
    pub os: Vec<String>,
    pub enabled: bool,
    pub exclusions: Vec<String>,
    pub agents: Vec<String>,
    pub links: Vec<Link>,
}

#[derive(Serialize)]
pub struct Link {
    pub path: PathBuf,
    pub target: Option<PathBuf>,
    pub recorded_target: Option<PathBuf>,
    pub owned: bool,
    pub status: LinkStatus,
}

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LinkStatus {
    Missing,
    Unmanaged,
    Broken,
    DifferentTarget,
    Installed,
    LocalSource,
}

impl std::fmt::Display for LinkStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Missing => "missing",
            Self::Unmanaged => "unmanaged",
            Self::Broken => "broken (owned)",
            Self::DifferentTarget => "different target (owned)",
            Self::Installed => "installed (owned)",
            Self::LocalSource => "local source (direct)",
        })
    }
}

fn has_skill(path: &Path, local_source: bool) -> bool {
    if local_source {
        path.join("SKILL.md").is_file()
    } else {
        fs::symlink_metadata(path.join("SKILL.md")).is_ok_and(|m| m.is_file())
    }
}

fn inspect(
    path: PathBuf,
    records: &BTreeMap<PathBuf, PathBuf>,
    expected: Option<&Path>,
    local_source: bool,
) -> Result<Link> {
    let metadata = match fs::symlink_metadata(&path) {
        Ok(meta) => Some(meta),
        Err(err) if err.kind() == ErrorKind::NotFound => None,
        Err(err) => return Err(err).with_context(|| format!("checking {}", path.display())),
    };
    let target = metadata
        .as_ref()
        .filter(|m| m.file_type().is_symlink())
        .map(|_| fs::read_link(&path))
        .transpose()?;
    let recorded_target = records.get(&path).cloned();
    let owned = target.is_some() && target == recorded_target;
    let status = if metadata.is_none() {
        LinkStatus::Missing
    } else if local_source && expected == Some(path.as_path()) {
        LinkStatus::LocalSource
    } else if !owned {
        LinkStatus::Unmanaged
    } else if !has_skill(&path, local_source) {
        LinkStatus::Broken
    } else if expected.is_some_and(|e| target.as_deref() != Some(e)) {
        LinkStatus::DifferentTarget
    } else {
        LinkStatus::Installed
    };
    Ok(Link {
        path,
        target,
        recorded_target,
        owned,
        status,
    })
}

pub fn collect(paths: &Paths, active: &Active, name: &str) -> Result<Report> {
    manifest::validate_name(name)?;
    let mut manifest = if paths.manifest().try_exists()? {
        manifest::load(paths)?
    } else {
        manifest::parse("", paths)?
    };
    let explicit = manifest.skills.iter().any(|s| s.name == name);
    let lock = Lock::load(&paths.lock())?;
    xcode::append_skills(paths, &mut manifest, &lock.xcode)?;
    let records = sync::owned_links(paths)?;
    let mut report = Report {
        name: name.into(),
        manifest: paths.manifest(),
        origin: "unmanaged".into(),
        source: None,
        git_ref: None,
        subdir: None,
        locked: None,
        lock_matches: false,
        xcode: None,
        content_path: None,
        content_available: false,
        profiles: vec![],
        active_profiles: active.names.clone(),
        profile_source: active.from.to_string(),
        os: vec![],
        enabled: false,
        exclusions: vec![],
        agents: vec![],
        links: vec![],
    };
    let mut destinations = BTreeSet::new();
    if let Some(skill) = manifest.skills.iter().find(|s| s.name == name) {
        report.profiles = skill.profiles.clone();
        report.os = skill.os.clone();
        report.enabled = skill.enabled(active);
        if !manifest::matches_os(&skill.os) {
            report.exclusions.push(format!(
                "OS {} is outside [{}]",
                std::env::consts::OS,
                skill.os.join(", ")
            ));
        }
        if !active.includes(&skill.profiles) {
            report.exclusions.push(format!(
                "active profiles do not match [{}]",
                skill.profiles.join(", ")
            ));
        }
        report.agents = skill.agents.iter().map(|a| a.id.clone()).collect();
        destinations.extend(skill.agents.iter().map(|a| a.dir.join(name)));
        if !explicit {
            report.origin = "Xcode".into();
            report.xcode = lock.xcode.get(name).cloned();
            report.content_path = report
                .xcode
                .as_ref()
                .map(|e| xcode::skill_dir(paths, name, e));
        } else {
            match &skill.origin {
                Origin::Local(dir) => {
                    report.origin = "local".into();
                    report.content_path = Some(dir.clone());
                }
                Origin::Git(origin) => {
                    report.origin = "git".into();
                    report.source = Some(origin.source.clone());
                    report.git_ref = origin.git_ref.clone();
                    report.subdir = origin.subdir.clone();
                    report.locked = lock.skills.get(name).cloned();
                    if let Some(entry) = &report.locked {
                        report.lock_matches = entry.matches(origin);
                        let url = manifest::source_url(&entry.source)?;
                        report.content_path = Some(
                            paths
                                .store()
                                .join(git::cache_key(&url))
                                .join(&entry.commit)
                                .join(sync::store_name(&entry.subdir)),
                        );
                    }
                }
            }
        }
    }
    // Include existing copies in every configured destination, and old recorded links
    // after an agent override changes. A state record alone never proves ownership.
    for agent in manifest
        .agents
        .iter()
        .chain(manifest.skills.iter().flat_map(|s| &s.agents))
    {
        let path = agent.dir.join(name);
        match fs::symlink_metadata(&path) {
            Ok(_) => {
                destinations.insert(path);
            }
            Err(err) if err.kind() == ErrorKind::NotFound => {}
            Err(err) => {
                return Err(err).with_context(|| format!("checking {}", paths.tilde(&path)));
            }
        }
    }
    destinations.extend(
        records
            .keys()
            .filter(|p| p.file_name().is_some_and(|n| n == name))
            .cloned(),
    );
    for path in destinations {
        report.links.push(inspect(
            path,
            &records,
            report.content_path.as_deref(),
            report.origin == "local",
        )?);
    }
    if report.origin == "unmanaged" && report.links.is_empty() {
        bail!(
            "unknown skill `{name}`: not configured or found in configured agent directories (Xcode wildcard names require a prior sync)"
        );
    }
    report.content_available = report
        .content_path
        .as_ref()
        .is_some_and(|p| has_skill(p, report.origin == "local"));
    Ok(report)
}

impl Report {
    pub fn print(&self, paths: &Paths) {
        println!(
            "Skill: {}\nOrigin: {}\nManifest: {}",
            self.name,
            self.origin,
            paths.tilde(&self.manifest)
        );
        if let Some(source) = &self.source {
            println!(
                "Source: {source}\nRef: {}\nSubdirectory: {}",
                self.git_ref.as_deref().unwrap_or("default branch"),
                self.subdir.as_deref().unwrap_or("auto-discover")
            );
            if let Some(entry) = &self.locked {
                println!(
                    "Pinned commit: {}\nPinned source: {}\nPinned ref: {}\nPinned subdirectory: {}\nLock: {}",
                    entry.commit,
                    entry.source,
                    entry.git_ref.as_deref().unwrap_or("default branch"),
                    if entry.subdir.is_empty() {
                        "."
                    } else {
                        &entry.subdir
                    },
                    if self.lock_matches {
                        "matches manifest"
                    } else {
                        "stale (does not match manifest)"
                    }
                );
            } else {
                println!("Lock: not locked");
            }
        }
        if self.origin == "Xcode" {
            if let Some(entry) = &self.xcode {
                println!(
                    "Xcode version: {}\nContent digest: {}",
                    entry.version.replace('\n', " / "),
                    entry.digest
                );
            } else {
                println!("Xcode: not exported");
            }
        }
        if let Some(path) = &self.content_path {
            println!(
                "Content: {} ({})",
                paths.tilde(path),
                if self.content_available {
                    "available"
                } else {
                    "missing SKILL.md"
                }
            );
        }
        println!(
            "Agents: {}\nProfiles: {}\nActive profiles: {} ({})\nOS: {} (current: {})",
            self.agents.join(", "),
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
            std::env::consts::OS
        );
        println!(
            "Selection: {}",
            if self.origin == "unmanaged" {
                "not in manifest".into()
            } else if self.enabled {
                "included".into()
            } else {
                format!("excluded: {}", self.exclusions.join("; "))
            }
        );
        for link in &self.links {
            println!("Link: {} — {}", paths.tilde(&link.path), link.status);
            if let Some(target) = &link.target {
                println!("  Target: {}", paths.tilde(target));
            }
            if let Some(target) = &link.recorded_target {
                println!("  Recorded target: {}", paths.tilde(target));
            }
        }
    }
}
