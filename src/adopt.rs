use std::fs;
use std::path::Path;

use anyhow::{Context, Result, ensure};
use toml_edit::{InlineTable, value};

use crate::{SkillOptions, agents, manifest, paths::Paths, sync};

/// Snapshot first, then publish the manifest. Never claim the source installation.
pub fn run(
    paths: &Paths,
    name: &str,
    from: &str,
    options: &SkillOptions,
    dry_run: bool,
) -> Result<()> {
    manifest::validate_name(name)?;
    options.validate(paths)?;
    let source = agents::resolve(from, paths)?.dir.join(name);
    let owned = sync::owned_links(paths)?;
    ensure!(
        fs::read_link(&source)
            .ok()
            .as_ref()
            .is_none_or(|target| owned.get(&source) != Some(target)),
        "{} is already managed by skl",
        paths.tilde(&source)
    );
    let resolved = source
        .canonicalize()
        .with_context(|| format!("cannot open installed skill {}", paths.tilde(&source)))?;
    ensure!(
        resolved.is_dir(),
        "{} is not a skill directory",
        paths.tilde(&source)
    );
    let skill = resolved.join("SKILL.md");
    ensure!(
        fs::symlink_metadata(&skill).is_ok_and(|m| m.is_file()),
        "{} needs a regular SKILL.md",
        paths.tilde(&source)
    );
    // Reject special files and nested symlinks rather than making a partial or
    // machine-dependent copy. The top-level installed link is explicitly resolved.
    validate_tree(&resolved)?;
    let local = paths.config.join("local");
    if let Ok(metadata) = fs::symlink_metadata(&local) {
        ensure!(
            metadata.is_dir(),
            "{} must be a real directory",
            paths.tilde(&local)
        );
    }
    let destination = local.join(name);
    let config = std::path::absolute(&paths.config)?;
    let config = config.canonicalize().unwrap_or(config);
    ensure!(
        !config.join("local").join(name).starts_with(&resolved),
        "snapshot destination must not be inside the installed source"
    );
    ensure!(
        fs::symlink_metadata(&destination).is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound),
        "snapshot destination {} already exists or cannot be inspected",
        paths.tilde(&destination)
    );
    let mut doc = crate::read_manifest_doc(paths)?;
    let table = crate::skills_table(&mut doc)?;
    ensure!(
        !table.contains_key(name),
        "`{name}` is already in the manifest"
    );
    let mut entry = InlineTable::new();
    entry.insert("path", format!("local/{name}").into());
    let mut item = value(entry);
    options.apply(&mut item, [false; 3])?;
    table.insert(name, item);
    let text = doc.to_string();
    let mut manifest = manifest::parse(&text, paths)?;
    let lock = crate::lock::Lock::load(&paths.lock())?;
    for (alias, group) in &manifest.repositories {
        ensure!(
            lock.repositories
                .get(alias)
                .is_some_and(|entry| crate::repositories::matches(
                    group,
                    entry,
                    &manifest.agents,
                    paths
                )
                .unwrap_or(false)),
            "repository `{alias}` wildcard coverage is missing or stale; run `skl sync` before adopting"
        );
    }
    ensure!(
        !manifest
            .xcode
            .as_ref()
            .is_some_and(|config| config.skills.is_none() && !lock.xcode_all_complete),
        "Xcode wildcard selection has no complete cached export; run `skl sync` before adopting"
    );
    crate::repositories::append(paths, &mut manifest, &lock.repositories)?;
    crate::xcode::append_skills(paths, &mut manifest, &lock.xcode)?;
    println!(
        "{}snapshot {} -> {}",
        if dry_run { "Would " } else { "Will " },
        paths.tilde(&source),
        paths.tilde(&destination)
    );
    println!(
        "The original installation stays unchanged; existing paths remain conflicts. Review `skl sync --dry-run` before explicitly using `skl sync --force` to back them up and replace them."
    );
    if dry_run {
        return Ok(());
    }
    fs::create_dir_all(destination.parent().unwrap())?;
    let staging = tempfile::tempdir_in(destination.parent().unwrap())?;
    let content = staging.path().join("content");
    copy_tree(&resolved, &content)?;
    fs::rename(&content, &destination)
        .with_context(|| format!("publishing snapshot {}", paths.tilde(&destination)))?;
    if let Err(error) = crate::write_atomic(&paths.manifest(), text.as_bytes()) {
        fs::remove_dir_all(&destination).with_context(|| {
            format!(
                "removing unregistered snapshot {} after {error:#}",
                paths.tilde(&destination)
            )
        })?;
        return Err(error);
    }
    Ok(())
}

fn validate_tree(path: &Path) -> Result<()> {
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        ensure!(
            kind.is_file() || kind.is_dir(),
            "cannot snapshot {}: nested symlinks and special files are unsupported",
            entry.path().display()
        );
        if kind.is_dir() {
            validate_tree(&entry.path())?;
        }
    }
    Ok(())
}

fn copy_tree(source: &Path, destination: &Path) -> Result<()> {
    fs::create_dir(destination)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        if kind.is_dir() {
            copy_tree(&entry.path(), &destination.join(entry.file_name()))?;
        } else {
            ensure!(
                kind.is_file(),
                "source changed while snapshotting {}",
                entry.path().display()
            );
            fs::copy(entry.path(), destination.join(entry.file_name()))?;
        }
    }
    fs::set_permissions(destination, fs::metadata(source)?.permissions())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failed_manifest_publication_removes_snapshot() {
        let temp = tempfile::tempdir().unwrap();
        let mut paths = Paths::from_env().unwrap();
        paths.config = temp.path().join("config");
        paths.data = temp.path().join("data");
        let agent = temp.path().join("agent");
        fs::create_dir_all(agent.join("review")).unwrap();
        fs::write(agent.join("review/SKILL.md"), "content").unwrap();
        fs::create_dir_all(&paths.config).unwrap();
        // Force write_atomic's temporary write to fail after snapshot publication.
        fs::create_dir(
            paths
                .config
                .join(format!(".skills.toml.tmp-{}", std::process::id())),
        )
        .unwrap();
        assert!(
            run(
                &paths,
                "review",
                agent.to_str().unwrap(),
                &SkillOptions::default(),
                false
            )
            .is_err()
        );
        assert!(!paths.config.join("local/review").exists());
        assert!(!paths.manifest().exists());
        assert!(agent.join("review/SKILL.md").is_file());
    }
}
