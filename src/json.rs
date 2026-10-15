//! Versioned, single-document output for read-only inspection commands.
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Result;
use serde_json::{Value, json};

use crate::{doctor, info, lock::Lock, manifest, paths::Paths, profile::Active, sync, xcode};

fn emit(command: &str, status: &str, data: Value, diagnostics: Value) -> Result<()> {
    let text = serde_json::to_string(&json!({
        "schema_version": 1,
        "command": command,
        "status": status,
        "data": data,
        "diagnostics": diagnostics,
    }))?;
    println!("{text}");
    Ok(())
}

pub fn error(command: &str, error: &anyhow::Error) {
    // Strings and JSON values cannot fail serialization.
    let _ = emit(
        command,
        "error",
        Value::Null,
        json!([{
            "code": "runtime-error", "severity": "error", "skill": null, "path": null,
            "message": format!("{error:#}"),
            "remedy": "Correct the reported configuration or filesystem error and retry.",
        }]),
    );
}

fn absolute(path: &Path) -> Result<PathBuf> {
    Ok(std::path::absolute(path)?)
}

fn path_value(path: &Path) -> Result<Value> {
    Ok(serde_json::to_value(absolute(path)?)?)
}

fn report_value(mut report: info::Report) -> Result<Value> {
    report.manifest = absolute(&report.manifest)?;
    report.content_path = report.content_path.as_deref().map(absolute).transpose()?;
    for link in &mut report.links {
        link.path = absolute(&link.path)?;
        for value in [&mut link.target, &mut link.recorded_target]
            .into_iter()
            .flatten()
        {
            *value = absolute(&link.path.parent().unwrap().join(&*value))?;
        }
    }
    Ok(serde_json::to_value(report)?)
}

pub fn info(paths: &Paths, active: &Active, name: &str) -> Result<bool> {
    let report = info::collect(paths, active, name)?;
    // Info reports observations; an unhealthy installation is still a successful inspection.
    emit("info", "ok", report_value(report)?, json!([]))?;
    Ok(true)
}

pub fn doctor(paths: &Paths, active: &Active) -> Result<bool> {
    let diagnostics = doctor::collect(paths, active)?;
    let healthy = diagnostics.is_empty();
    let diagnostics = diagnostics
        .into_iter()
        .map(|d| {
            Ok(json!({
                "code": d.code, "severity": "error", "skill": d.skill,
                "path": d.path.as_deref().map(path_value).transpose()?,
                "message": d.message, "remedy": d.remedy,
            }))
        })
        .collect::<Result<Vec<_>>>()?;
    emit(
        "doctor",
        if healthy { "ok" } else { "unhealthy" },
        json!({
            "active_profiles": active.names,
            "profile_source": active.from.to_string(),
        }),
        json!(diagnostics),
    )?;
    Ok(healthy)
}

pub fn list(paths: &Paths, active: &Active) -> Result<bool> {
    let mut manifest = manifest::load(paths)?;
    let explicit: BTreeSet<_> = manifest.skills.iter().map(|s| s.name.clone()).collect();
    let lock = Lock::load(&paths.lock())?;
    let records = sync::owned_links(paths)?;
    xcode::append_skills(paths, &mut manifest, &lock.xcode)?;
    let mut diagnostics = vec![];
    if manifest.xcode.as_ref().is_some_and(|config| {
        config.skills.is_none() && !lock.xcode_all_complete && config.enabled(active)
    }) {
        diagnostics.push(json!({
            "code": "xcode-not-locked", "severity": "error", "skill": null,
            "path": path_value(&paths.lock())?,
            "message": "Xcode wildcard selection has no complete cached export",
            "remedy": "Run skl sync on a machine with Xcode configured.",
        }));
    }
    let mut skills = vec![];
    let mut sorted: Vec<_> = manifest.skills.iter().collect();
    sorted.sort_by(|a, b| a.name.cmp(&b.name));
    for skill in sorted {
        // Keep the same health/exit criteria as the human list command.
        if skill.enabled(active) {
            for agent in &skill.agents {
                let link = agent.dir.join(&skill.name);
                if !(link.is_symlink() && link.join("SKILL.md").is_file()) {
                    diagnostics.push(json!({
                        "code": "not-linked", "severity": "error", "skill": skill.name,
                        "path": path_value(&link)?,
                        "message": format!("skill is not linked for {}", agent.id),
                        "remedy": "Run skl sync to fix missing links.",
                    }));
                }
            }
        }
        skills.push(report_value(info::collect_loaded(
            paths,
            active,
            &skill.name,
            &manifest,
            &lock,
            &records,
            explicit.contains(&skill.name),
        )?)?);
    }
    let mut seen = BTreeSet::new();
    let mut unmanaged = vec![];
    for agent in manifest
        .agents
        .iter()
        .chain(manifest.skills.iter().flat_map(|s| &s.agents))
    {
        if !seen.insert(agent.dir.clone()) {
            continue;
        }
        // Match human list's best-effort directory discovery.
        let Ok(entries) = fs::read_dir(&agent.dir) else {
            continue;
        };
        let mut entries: Vec<_> = entries.flatten().map(|e| e.path()).collect();
        entries.sort();
        for path in entries {
            let name = crate::file_name(&path);
            if name.starts_with('.') {
                continue;
            }
            let target = fs::read_link(&path).ok();
            if target.is_some() && records.get(&path) == target.as_ref() {
                continue;
            }
            let kind = if target.is_some() {
                "symlink"
            } else if path.is_dir() {
                "directory"
            } else {
                "file"
            };
            let target = target
                .map(|t| path_value(&path.parent().unwrap().join(t)))
                .transpose()?;
            unmanaged.push(json!({
                "agent": agent.id, "name": name, "path": path_value(&path)?,
                "kind": kind, "target": target,
            }));
        }
    }
    unmanaged.sort_by(|a, b| a["path"].as_str().cmp(&b["path"].as_str()));
    let healthy = diagnostics.is_empty();
    emit(
        "list",
        if healthy { "ok" } else { "unhealthy" },
        json!({
            "skills": skills, "unmanaged": unmanaged,
            "active_profiles": active.names, "profile_source": active.from.to_string(),
        }),
        json!(diagnostics),
    )?;
    Ok(healthy)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::ffi::OsStringExt;

    #[test]
    fn non_utf8_paths_fail_serialization_without_panicking() {
        let path = PathBuf::from(std::ffi::OsString::from_vec(b"/invalid-\xff".to_vec()));
        assert!(path_value(&path).is_err());
    }
}
