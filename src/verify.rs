//! Opt-in, offline verification of cached skill contents. No installation writes.
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Read;
use std::os::unix::fs::PermissionsExt;
use std::path::{Component, Path, PathBuf};

use anyhow::{Context, Result, bail, ensure};
use serde::Serialize;
use serde_json::json;
use sha2::{Digest, Sha256};

use crate::{
    export,
    git::{self, Git},
    lock::Lock,
    manifest::{self, Origin},
    paths::Paths,
    profile::Active,
    repositories, sync,
};

#[derive(Serialize)]
struct Observation {
    name: String,
    source_kind: &'static str,
    /// For a plugin, the agents that install this copy.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    agents: Vec<String>,
    status: &'static str,
    path: Option<PathBuf>,
    message: String,
    changes: Vec<Change>,
}

#[derive(Serialize)]
struct Change {
    path: PathBuf,
    kind: &'static str,
}

#[derive(Serialize)]
struct Diagnostic {
    code: &'static str,
    severity: &'static str,
    skill: Option<String>,
    path: Option<PathBuf>,
    message: String,
    remedy: &'static str,
}

pub fn run(paths: &Paths, active: &Active, names: &[String], json_output: bool) -> Result<bool> {
    let mut manifest = manifest::load(paths)?;
    let lock = Lock::load(&paths.lock())?;
    repositories::append(paths, &mut manifest, &lock.repositories)?;
    let explicit: BTreeSet<_> = manifest.skills.iter().map(|s| s.name.clone()).collect();
    export::append(paths, &mut manifest, &lock.commands, &Default::default())?;
    for name in names {
        manifest::validate_name(name)?;
        ensure!(
            manifest.skills.iter().any(|s| s.name == *name)
                || manifest.plugins.iter().any(|p| p.name == *name),
            "unknown skill or plugin `{name}`"
        );
    }
    let mut diagnostics = Vec::new();
    if names.is_empty() {
        for alias in repositories::pending(paths, &manifest, &lock.repositories, active)? {
            diagnostics.push(Diagnostic {
                code: "baseline-unavailable",
                severity: "error",
                skill: None,
                path: Some(std::path::absolute(paths.lock())?),
                message: format!("source `{alias}` has missing or stale wildcard coverage"),
                remedy: "Run skl sync to populate the pinned baseline, then verify again.",
            });
        }
    }
    let git = Git::new(paths.repos());
    let mut observations = Vec::new();
    manifest.skills.sort_by(|a, b| a.name.cmp(&b.name));
    for skill in &manifest.skills {
        if !names.is_empty() && !names.contains(&skill.name) {
            continue;
        }
        let kind = match &skill.origin {
            Origin::Git(_) => "git",
            Origin::Command(_) => "command",
            Origin::Local(_) if !explicit.contains(&skill.name) => "app",
            Origin::Local(_) | Origin::OptionalLocal(_) => "local",
        };
        let mut observation = Observation {
            name: skill.name.clone(),
            source_kind: kind,
            agents: Vec::new(),
            status: "skipped",
            path: None,
            message: "disabled for the active profiles or operating system".into(),
            changes: Vec::new(),
        };
        if skill.enabled(active) {
            let verified = (|| -> Result<Vec<Change>> {
                match &skill.origin {
                    Origin::Command(origin) => {
                        let entry = lock
                            .commands
                            .get(&skill.name)
                            .filter(|e| e.producer == *origin)
                            .context("no matching command snapshot")?;
                        let target = crate::app::skill_dir(paths, &skill.name, entry);
                        observation.path = Some(std::path::absolute(&target)?);
                        ensure_directory_chain(&paths.store(), &target)?;
                        ensure!(
                            export::content_digest(&target)? == entry.snapshot.digest,
                            "command snapshot digest mismatch"
                        );
                        Ok(Vec::new())
                    }
                    Origin::Git(origin) => {
                        let entry = lock
                            .skills
                            .get(&skill.name)
                            .filter(|entry| entry.matches(origin))
                            .context("no matching locked Git baseline")?;
                        ensure!(
                            !entry.subdir.starts_with('/')
                                && (entry.subdir.is_empty()
                                    || entry
                                        .subdir
                                        .split('/')
                                        .all(|p| !matches!(p, "" | "." | ".."))),
                            "invalid locked skill directory"
                        );
                        let target = paths
                            .store()
                            .join(git::cache_key(&origin.url))
                            .join(&entry.commit)
                            .join(sync::store_name(&entry.subdir));
                        observation.path = Some(std::path::absolute(&target)?);
                        // Keep baseline exports outside persistent data, even when data doesn't exist.
                        let temporary = tempfile::tempdir()?;
                        let baseline = temporary.path().join("baseline");
                        git.export_cached(&origin.url, &entry.commit, &entry.subdir, &baseline)
                            .context("locked Git baseline is unavailable in the existing repository cache")?;
                        let expected = tree(&baseline)?;
                        ensure_directory_chain(&paths.store(), &target)?;
                        compare(&expected, &tree(&target)?)
                    }
                    Origin::Local(target) if kind == "app" => {
                        observation.path = Some(std::path::absolute(target)?);
                        let entry = export::member(&lock, &skill.name)
                            .context("no locked app snapshot digest")?;
                        ensure_directory_chain(&paths.store(), target)?;
                        let digest = export::content_digest(target)?;
                        Ok(if digest == entry.snapshot.digest {
                            Vec::new()
                        } else {
                            vec![Change {
                                path: PathBuf::from("."),
                                kind: "digest-changed",
                            }]
                        })
                    }
                    Origin::Local(_) | Origin::OptionalLocal(_) => {
                        bail!("local skills have no pinned content baseline");
                    }
                }
            })();
            match verified {
                Ok(changes) => {
                    observation.status = if changes.is_empty() {
                        "verified"
                    } else {
                        "drift"
                    };
                    observation.message = if changes.is_empty() {
                        "cached content matches the pinned baseline"
                    } else {
                        "cached content differs from the pinned baseline"
                    }
                    .into();
                    observation.changes = changes;
                }
                Err(error) => {
                    observation.status = if kind == "local" {
                        "skipped"
                    } else {
                        "unavailable"
                    };
                    observation.message = format!("{error:#}");
                }
            }
        }
        if matches!(observation.status, "drift" | "unavailable") {
            diagnostics.push(Diagnostic { code: if observation.status == "drift" { "content-drift" } else { "baseline-unavailable" }, severity: "error", skill: Some(skill.name.clone()),
                path: observation.path.clone(),
                message: observation.message.clone(), remedy: "Inspect the reported content; restore the cache from a trusted copy or move the affected skill directory aside and run skl sync. Verification never repairs files." });
        }
        observations.push(observation);
    }
    let state = crate::plugins::State::load(paths)?;
    manifest.plugins.sort_by(|a, b| a.name.cmp(&b.name));
    for plugin in &manifest.plugins {
        if !names.is_empty() && !names.contains(&plugin.name) {
            continue;
        }
        for mut observation in verify_plugin(paths, active, &git, &lock, &state, plugin)? {
            if matches!(observation.status, "drift" | "unavailable") {
                diagnostics.push(Diagnostic {
                    code: if observation.status == "drift" {
                        "content-drift"
                    } else {
                        "baseline-unavailable"
                    },
                    severity: "error",
                    skill: Some(plugin.name.clone()),
                    path: observation.path.clone(),
                    message: observation.message.clone(),
                    remedy: "Inspect the reported content; restore the cache from a trusted copy or move the affected plugin directory aside and run skl sync. Verification never repairs files.",
                });
            }
            observation.name = plugin.name.clone();
            observations.push(observation);
        }
    }
    let healthy = diagnostics.is_empty();
    if json_output {
        let observations = serde_json::to_value(observations)?;
        let diagnostics = serde_json::to_value(diagnostics)?;
        crate::json::emit(
            "verify",
            if healthy { "ok" } else { "unhealthy" },
            json!({
                "skills": observations, "active_profiles": active.names, "profile_source": active.from.to_string(),
            }),
            json!(diagnostics),
        )?;
    } else {
        for observation in observations {
            let location = observation
                .path
                .as_ref()
                .map(|p| format!(" ({})", paths.tilde(p)))
                .unwrap_or_default();
            let agents = if observation.agents.is_empty() {
                String::new()
            } else {
                format!(" [{}]", observation.agents.join(", "))
            };
            println!(
                "{}: {}{agents}{}: {}",
                observation.status, observation.name, location, observation.message
            );
            for change in observation.changes {
                println!("  {}: {}", change.kind, change.path.display());
            }
        }
        for diagnostic in diagnostics.iter().filter(|d| d.skill.is_none()) {
            println!("{}: {}", diagnostic.code, diagnostic.message);
        }
    }
    Ok(healthy)
}

/// Compares a plugin's store folders, and Cursor's copies of them, with the locked
/// commit. Claude Code reads the store folder in place; Codex keeps its own copy,
/// which skl can't see.
fn verify_plugin(
    paths: &Paths,
    active: &Active,
    git: &Git,
    lock: &Lock,
    state: &crate::plugins::State,
    plugin: &crate::plugins::Plugin,
) -> Result<Vec<Observation>> {
    let observation = |agents: Vec<String>, status, path, message: String| Observation {
        name: plugin.name.clone(),
        source_kind: "plugin",
        agents,
        status,
        path,
        message,
        changes: Vec::new(),
    };
    if !plugin.enabled(active) {
        return Ok(vec![observation(
            Vec::new(),
            "skipped",
            None,
            "disabled for the active profiles or operating system".into(),
        )]);
    }
    let Some(entry) = lock.plugins.get(&plugin.name).filter(|e| e.matches(plugin)) else {
        return Ok(vec![observation(
            Vec::new(),
            "unavailable",
            None,
            "no matching locked Git baseline".into(),
        )]);
    };
    // Folders shared by several agents are checked once.
    let mut folders: BTreeMap<&str, Vec<String>> = BTreeMap::new();
    for harness in &plugin.agents {
        if let Some(dir) = entry.subdirs.get(harness.id()) {
            folders.entry(dir).or_default().push(harness.id().into());
        }
    }
    let mut out = Vec::new();
    for (dir, agents) in folders {
        let target = crate::plugins::store_dir(paths, &plugin.url, &entry.commit, dir);
        let mut copies = Vec::new();
        if agents.iter().any(|a| a == "cursor")
            && let Some(record) = state
                .get("cursor", &plugin.name)
                .filter(|r| r.target == target)
        {
            copies.push(crate::plugins::local_dir(paths).join(&record.id));
        }
        let temporary = tempfile::tempdir()?;
        let baseline = temporary.path().join("baseline");
        let expected = git
            .export_cached(&plugin.url, &entry.commit, dir, &baseline)
            .context("locked Git baseline is unavailable in the existing repository cache")
            .and_then(|()| tree(&baseline));
        for (path, agents, in_store) in std::iter::once((target, agents.clone(), true)).chain(
            copies
                .into_iter()
                .map(|c| (c, vec!["cursor".to_owned()], false)),
        ) {
            let mut item = observation(
                agents,
                "verified",
                Some(std::path::absolute(&path)?),
                String::new(),
            );
            let result = expected
                .as_ref()
                .map_err(|e| anyhow::anyhow!("{e:#}"))
                .and_then(|expected| {
                    if in_store {
                        ensure_directory_chain(&paths.store(), &path)?;
                    } else {
                        ensure!(
                            fs::symlink_metadata(&path)?.is_dir(),
                            "{} is not a directory",
                            path.display()
                        );
                    }
                    compare(expected, &tree(&path)?)
                });
            match result {
                Ok(changes) if changes.is_empty() => {
                    item.message = "cached content matches the pinned baseline".into();
                }
                Ok(changes) => {
                    item.status = "drift";
                    item.message = "cached content differs from the pinned baseline".into();
                    item.changes = changes;
                }
                Err(error) => {
                    item.status = "unavailable";
                    item.message = format!("{error:#}");
                }
            }
            out.push(item);
        }
    }
    Ok(out)
}

/// Reject redirects at the skill root or any store ancestor. Recursion below never
/// follows symlinks: links are values to compare, not directories to descend into.
fn ensure_directory_chain(store: &Path, target: &Path) -> Result<()> {
    let relative = target.strip_prefix(store)?;
    let mut path = store.to_path_buf();
    ensure!(
        fs::symlink_metadata(&path)?.is_dir(),
        "cache directory is missing or is a symlink: {}",
        path.display()
    );
    for component in relative.components() {
        ensure!(
            matches!(component, Component::Normal(_)),
            "invalid cache directory"
        );
        path.push(component);
        ensure!(
            fs::symlink_metadata(&path)?.is_dir(),
            "cache directory is missing or is a symlink: {}",
            path.display()
        );
    }
    Ok(())
}

#[derive(PartialEq)]
enum Node {
    Directory,
    File { digest: Vec<u8>, executable: u32 },
    Symlink(PathBuf),
    Unsupported,
}

fn tree(root: &Path) -> Result<BTreeMap<PathBuf, Node>> {
    fn visit(root: &Path, dir: &Path, out: &mut BTreeMap<PathBuf, Node>) -> Result<()> {
        for child in fs::read_dir(dir).with_context(|| format!("reading {}", dir.display()))? {
            let path = child?.path();
            let metadata = fs::symlink_metadata(&path)?;
            let node = if metadata.is_dir() {
                Node::Directory
            } else if metadata.file_type().is_symlink() {
                Node::Symlink(fs::read_link(&path)?)
            } else if metadata.is_file() {
                let mut file = fs::File::open(&path)?;
                let mut hash = Sha256::new();
                let mut buffer = [0; 64 * 1024];
                loop {
                    let read = file.read(&mut buffer)?;
                    if read == 0 {
                        break;
                    }
                    hash.update(&buffer[..read]);
                }
                Node::File {
                    digest: hash.finalize().to_vec(),
                    executable: metadata.permissions().mode() & 0o111,
                }
            } else {
                Node::Unsupported
            };
            out.insert(path.strip_prefix(root)?.to_path_buf(), node);
            if metadata.is_dir() {
                visit(root, &path, out)?;
            }
        }
        Ok(())
    }
    let mut out = BTreeMap::new();
    visit(root, root, &mut out)?;
    Ok(out)
}

fn compare(
    expected: &BTreeMap<PathBuf, Node>,
    actual: &BTreeMap<PathBuf, Node>,
) -> Result<Vec<Change>> {
    let names: BTreeSet<_> = expected.keys().chain(actual.keys()).collect();
    let mut changes = Vec::new();
    for path in names {
        let kind = match (expected.get(path), actual.get(path)) {
            (None, Some(_)) => "added",
            (Some(_), None) => "deleted",
            (Some(left), Some(right)) if left != right => "changed",
            _ => continue,
        };
        changes.push(Change {
            path: path.clone(),
            kind,
        });
    }
    Ok(changes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::ffi::OsStringExt;

    #[test]
    fn non_utf8_change_paths_return_serialization_errors() {
        let change = Change {
            path: PathBuf::from(std::ffi::OsString::from_vec(b"invalid-\xff".to_vec())),
            kind: "added",
        };
        assert!(serde_json::to_value(change).is_err());
    }
}
