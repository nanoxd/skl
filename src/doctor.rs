//! Offline, read-only installation diagnostics. Collection is separate from output.
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{ErrorKind, Read};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::{
    git,
    lock::Lock,
    manifest::{self, Origin},
    paths::Paths,
    profile::Active,
    sync,
};

pub struct Diagnostic {
    pub code: &'static str,
    pub skill: Option<String>,
    pub path: Option<PathBuf>,
    pub message: String,
    pub remedy: &'static str,
}

pub fn run(paths: &Paths, active: &Active) -> Result<bool> {
    let diagnostics = collect(paths, active)?;
    for d in &diagnostics {
        let subject = d.skill.as_deref().unwrap_or("installation");
        let location = d
            .path
            .as_ref()
            .map(|p| format!(" ({})", paths.tilde(p)))
            .unwrap_or_default();
        println!("{}: {subject}{location}: {}", d.code, d.message);
        println!("  {}", d.remedy);
    }
    if diagnostics.is_empty() {
        println!("No problems found for the active profiles and operating system.");
    } else {
        println!(
            "{} problem(s) found. No files were changed.",
            diagnostics.len()
        );
    }
    Ok(diagnostics.is_empty())
}

pub fn collect(paths: &Paths, active: &Active) -> Result<Vec<Diagnostic>> {
    let mut manifest = manifest::load(paths)
        .context("check skills.toml and fix its configuration before running doctor again")?;
    let lock = Lock::load(&paths.lock()).context(
        "check skills.lock; restore a valid lockfile from dotfiles before running doctor again",
    )?;
    crate::repositories::append(paths, &mut manifest, &lock.repositories)?;
    let recorded = sync::owned_links(paths)
        .context("check state.toml; restore a valid state file before running doctor again")?;
    let mut out = Vec::new();
    for alias in crate::repositories::pending(paths, &manifest, &lock.repositories, active)? {
        issue(
            &mut out,
            "source-not-locked",
            None,
            Some(paths.lock()),
            format!("source `{alias}` wildcard coverage is missing or stale"),
            "Run skl sync to discover the source's skills.",
        );
    }
    for name in lock.skills.keys() {
        if !manifest.skills.iter().any(|skill| {
            skill.name == *name
                && (!skill.enabled(active) || matches!(skill.origin, Origin::Git(_)))
        }) {
            issue(
                &mut out,
                "stale-lock",
                Some(name),
                Some(paths.lock()),
                "git lock entry is no longer selected by the manifest".into(),
                "Run skl sync to reconcile the lockfile.",
            );
        }
    }
    for (name, entry) in &lock.commands {
        let Some(alias) = &entry.repository else {
            continue;
        };
        if manifest
            .apps
            .get(alias)
            .is_none_or(|app| app.enabled(active) && !app.selects(name))
        {
            issue(
                &mut out,
                "stale-lock",
                Some(name),
                Some(paths.lock()),
                format!("source `{alias}` lock entry is no longer selected by the manifest"),
                "Run skl sync to reconcile the lockfile.",
            );
        }
    }
    for (alias, coverage) in &lock.repositories {
        if coverage.producer.is_some()
            && manifest
                .apps
                .get(alias)
                .is_none_or(|app| app.enabled(active) && app.skills.is_some())
        {
            issue(
                &mut out,
                "stale-lock",
                None,
                Some(paths.lock()),
                format!("source `{alias}` wildcard coverage no longer matches the selection"),
                "Run skl sync to reconcile the lockfile.",
            );
        }
    }
    let local_names: BTreeSet<_> = manifest
        .skills
        .iter()
        .map(|skill| skill.name.clone())
        .collect();

    if manifest
        .skills
        .iter()
        .any(|s| s.enabled(active) && matches!(s.origin, Origin::Git(_)))
    {
        for tool in ["git", "tar"] {
            if !available(tool) {
                issue(
                    &mut out,
                    "missing-tool",
                    None,
                    None,
                    format!("`{tool}` is not executable on PATH"),
                    "Install the required tool or correct PATH before running skl sync.",
                );
            }
        }
    }
    for (alias, app) in &manifest.apps {
        let tool = &app.producer.command[0];
        if app.enabled(active) && !app.is_empty() && !available(tool) {
            issue(
                &mut out,
                "missing-tool",
                None,
                None,
                format!("`{tool}` for source `{alias}` is not executable on PATH"),
                "Install the app before refreshing its skills; cached skills can still be used with skl sync --locked.",
            );
        }
    }
    crate::export::append(paths, &mut manifest, &lock.commands, &Default::default())?;
    let mut expected = BTreeMap::new();
    let mut selected = BTreeSet::new();
    for skill in manifest.skills.iter().filter(|s| s.enabled(active)) {
        for agent in &skill.agents {
            selected.insert(agent.dir.join(&skill.name));
        }
        let (target, cached) = match &skill.origin {
            Origin::Local(dir) | Origin::OptionalLocal(dir) => {
                (dir.clone(), !local_names.contains(&skill.name))
            }
            Origin::Command(_) => {
                let Some(entry) = lock.commands.get(&skill.name) else {
                    continue;
                };
                (crate::app::skill_dir(paths, &skill.name, entry), true)
            }
            Origin::Git(origin) => {
                let Some(entry) = lock.skills.get(&skill.name).filter(|e| e.matches(origin)) else {
                    issue(
                        &mut out,
                        "not-locked",
                        Some(&skill.name),
                        Some(paths.lock()),
                        "no matching lock entry".into(),
                        "Run skl sync to resolve the manifest and update the lockfile.",
                    );
                    continue;
                };
                (
                    paths
                        .store()
                        .join(git::cache_key(&origin.url))
                        .join(&entry.commit)
                        .join(sync::store_name(&entry.subdir)),
                    true,
                )
            }
        };
        let skill_file = target.join("SKILL.md");
        let metadata = if cached {
            fs::symlink_metadata(&skill_file)
        } else {
            fs::metadata(&skill_file)
        };
        let readable = metadata.is_ok_and(|m| m.is_file())
            && fs::File::open(&skill_file)
                .and_then(|mut file| file.read(&mut [0; 1]))
                .is_ok();
        if !readable {
            issue(
                &mut out,
                if cached {
                    "invalid-cache"
                } else {
                    "missing-local-skill"
                },
                Some(&skill.name),
                Some(skill_file),
                "SKILL.md is missing, unreadable, or not a regular file".into(),
                if cached {
                    "Restore the cached skill from a trusted copy, or move its skill directory aside and run skl sync to reinstall it."
                } else {
                    "Restore a readable SKILL.md at the configured local path, check permissions, or correct the manifest path."
                },
            );
        }
        for agent in &skill.agents {
            let link = agent.dir.join(&skill.name);
            if link != target {
                expected.insert(link, (target.clone(), skill.name.clone()));
            }
        }
    }
    for (path, (target, name)) in &expected {
        check_link(&mut out, path, target, recorded.get(path), Some(name));
    }
    for (path, target) in &recorded {
        if expected.contains_key(path) {
            continue;
        }
        if !selected.contains(path) {
            issue(
                &mut out,
                "stale-state",
                None,
                Some(path.clone()),
                "recorded link is no longer selected by the manifest, profiles, or OS".into(),
                "Run skl sync to reconcile stale state; only links that still match their recorded targets will be removed.",
            );
        }
        check_link(&mut out, path, target, Some(target), None);
    }
    Ok(out)
}

fn check_link(
    out: &mut Vec<Diagnostic>,
    path: &Path,
    desired: &Path,
    recorded: Option<&PathBuf>,
    skill: Option<&str>,
) {
    match fs::symlink_metadata(path) {
        Err(e) if e.kind() == ErrorKind::NotFound => issue(
            out,
            "missing-link",
            skill,
            Some(path.into()),
            "link is missing".into(),
            "Run skl sync to create missing links.",
        ),
        Err(e) => issue(
            out,
            "unreadable-link",
            skill,
            Some(path.into()),
            e.to_string(),
            "Check permissions on the link and its parent directories.",
        ),
        Ok(_) => match fs::read_link(path) {
            Ok(actual) if actual == desired || recorded == Some(&actual) => {
                if actual != desired {
                    issue(
                        out,
                        "outdated-link",
                        skill,
                        Some(path.into()),
                        "owned link points to an older installation".into(),
                        "Run skl sync to repoint this owned link.",
                    );
                }
                if !path.exists() {
                    issue(
                        out,
                        "broken-link",
                        skill,
                        Some(path.into()),
                        "link target is missing or inaccessible".into(),
                        "Restore the skill target and check its permissions, then run skl sync.",
                    );
                }
            }
            _ => issue(
                out,
                "link-conflict",
                skill,
                Some(path.into()),
                "path is not an owned link with its recorded target".into(),
                "Move this user-owned path aside, or use skl sync --force to back it up before replacement.",
            ),
        },
    }
}

fn available(tool: &str) -> bool {
    std::env::var_os("PATH").is_some_and(|path| {
        std::env::split_paths(&path).any(|dir| {
            fs::metadata(dir.join(tool))
                .is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
        })
    })
}

fn issue(
    out: &mut Vec<Diagnostic>,
    code: &'static str,
    skill: Option<&str>,
    path: Option<PathBuf>,
    message: String,
    remedy: &'static str,
) {
    out.push(Diagnostic {
        code,
        skill: skill.map(str::to_owned),
        path,
        message,
        remedy,
    });
}
