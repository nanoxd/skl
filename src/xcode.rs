//! Xcode exports are machine-generated snapshots, not reproducible git sources.
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};
use sha2::{Digest, Sha256};

use crate::git;
use crate::lock::XcodeSkill;
use crate::manifest::{self, Manifest, Origin, Skill, Xcode};
use crate::paths::Paths;

pub fn skill_dir(paths: &Paths, name: &str, entry: &XcodeSkill) -> PathBuf {
    paths.store().join("xcode").join(&entry.digest).join(name)
}

pub fn append_skills(
    paths: &Paths,
    manifest: &mut Manifest,
    entries: &BTreeMap<String, XcodeSkill>,
) -> Result<()> {
    let Some(config) = &manifest.xcode else {
        return Ok(());
    };
    let mut names: Vec<String> = entries
        .keys()
        .filter(|n| config.selects(n))
        .cloned()
        .collect();
    if let Some(selected) = &config.skills {
        for name in selected {
            if !names.contains(name) {
                names.push(name.clone());
            }
        }
    }
    for name in names {
        if manifest.skills.iter().any(|s| s.name == name) {
            bail!("Xcode skill `{name}` conflicts with [skills]; choose only one source");
        }
        let dir = entries
            .get(&name)
            .map(|entry| skill_dir(paths, &name, entry))
            .unwrap_or_else(|| paths.store().join("xcode/unavailable").join(&name));
        manifest.skills.push(Skill {
            name,
            origin: Origin::Local(dir),
            agents: config.agents.clone(),
            profiles: config.profiles.clone(),
            os: config.os.clone(),
        });
    }
    Ok(())
}

pub struct Export {
    _stage: tempfile::TempDir,
    version: String,
    skills: BTreeMap<String, PathBuf>,
}

fn command(program: &str, args: &[&std::ffi::OsStr]) -> Result<String> {
    let output = Command::new(program)
        .args(args)
        .output()
        .with_context(|| format!("running {program}; install and finish setting up Xcode first"))?;
    if !output.status.success() {
        bail!(
            "{program} failed; finish setting up Xcode and check the selected developer directory: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(String::from_utf8(output.stdout)?.trim().to_string())
}

impl Export {
    pub fn run() -> Result<Self> {
        let version = command("xcrun", &["xcodebuild".as_ref(), "-version".as_ref()])?;
        if version.is_empty() {
            bail!("Xcode returned an empty version");
        }
        let stage = tempfile::tempdir().context("creating Xcode export staging directory")?;
        let dest = stage.path().join("skills");
        command(
            "xcrun",
            &[
                "agent".as_ref(),
                "skills".as_ref(),
                "export".as_ref(),
                dest.as_os_str(),
            ],
        )?;
        let mut skills = BTreeMap::new();
        discover(&dest, &mut skills)?;
        Ok(Self {
            _stage: stage,
            version,
            skills,
        })
    }

    pub fn covers_all(&self, entries: &BTreeMap<String, XcodeSkill>) -> bool {
        self.skills.keys().eq(entries.keys())
    }

    pub fn install(
        &self,
        paths: &Paths,
        config: &Xcode,
        old: &BTreeMap<String, XcodeSkill>,
        reserved: &[Skill],
        update: Option<&Vec<String>>,
    ) -> Result<(BTreeMap<String, XcodeSkill>, Vec<String>)> {
        let mut names: Vec<String> = config
            .skills
            .clone()
            .unwrap_or_else(|| self.skills.keys().cloned().collect());
        if let Some(requested) = update.filter(|names| !names.is_empty()) {
            for name in requested {
                if !reserved.iter().any(|s| &s.name == name) && !names.contains(name) {
                    bail!("`{name}` isn't in the manifest or selected Xcode exports");
                }
            }
            for name in old.keys().filter(|n| config.selects(n)) {
                if !names.contains(name) {
                    names.push(name.clone());
                }
            }
        }
        for name in &names {
            if reserved.iter().any(|s| &s.name == name) {
                bail!("Xcode skill `{name}` conflicts with [skills]; choose only one source");
            }
        }
        let mut entries = BTreeMap::new();
        let mut errors = Vec::new();
        for name in names {
            if update.is_some_and(|names| !names.is_empty() && !names.contains(&name)) {
                if let Some(entry) = old.get(&name) {
                    entries.insert(name, entry.clone());
                }
                continue;
            }
            let result = (|| -> Result<XcodeSkill> {
                let source = self.skills.get(&name).with_context(|| format!("Xcode no longer exports `{name}`; update [xcode].skills to remove or rename it"))?;
                let skill_file = source.join("SKILL.md");
                let original = fs::read_to_string(&skill_file)?;
                let normalized = normalize_frontmatter(&original);
                if normalized != original {
                    let permissions = fs::metadata(&skill_file)?.permissions();
                    crate::write_atomic(&skill_file, normalized.as_bytes())?;
                    fs::set_permissions(&skill_file, permissions)?;
                }
                let entry = XcodeSkill {
                    version: self.version.clone(),
                    digest: content_digest(source)?,
                };
                let dest = skill_dir(paths, &name, &entry);
                if !dest.exists() {
                    let parent = dest.parent().unwrap();
                    fs::create_dir_all(parent)?;
                    let stage = tempfile::tempdir_in(parent)?;
                    copy_tree(source, stage.path())?;
                    fs::rename(stage.path(), &dest)?;
                }
                Ok(entry)
            })();
            match result {
                Ok(entry) => {
                    entries.insert(name, entry);
                }
                Err(err) => {
                    errors.push(format!("{name}: {err:#}"));
                    if let Some(entry) = old.get(&name) {
                        entries.insert(name, entry.clone());
                    }
                }
            }
        }
        Ok((entries, errors))
    }
}

fn children(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut paths = fs::read_dir(dir)
        .with_context(|| format!("reading Xcode export {}", dir.display()))?
        .map(|e| e.map(|e| e.path()))
        .collect::<std::io::Result<Vec<_>>>()?;
    paths.sort();
    Ok(paths)
}

fn discover(dir: &Path, found: &mut BTreeMap<String, PathBuf>) -> Result<()> {
    let skill = dir.join("SKILL.md");
    if let Ok(meta) = fs::symlink_metadata(&skill) {
        if !meta.is_file() {
            bail!("{} must be a regular file", skill.display());
        }
        let text = fs::read_to_string(&skill)?;
        let name = git::frontmatter_name(&text)
            .unwrap_or_else(|| dir.file_name().unwrap().to_string_lossy().into_owned());
        manifest::validate_name(&name)?;
        if found.insert(name.clone(), dir.to_owned()).is_some() {
            bail!("duplicate Xcode skill `{name}`");
        }
        return Ok(());
    }
    for path in children(dir)? {
        let meta = fs::symlink_metadata(&path)?;
        if meta.is_dir() {
            discover(&path, found)?;
        } else if meta.file_type().is_symlink() {
            bail!("Xcode export contains symlink {}", path.display());
        }
    }
    Ok(())
}

/// Digest a cached export with the same content, type, and mode encoding as installation.
pub fn content_digest(root: &Path) -> Result<String> {
    let mut hash = Sha256::new();
    hash_tree(root, root, &mut hash)?;
    Ok(format!("{:x}", hash.finalize()))
}

fn hash_tree(root: &Path, dir: &Path, hash: &mut Sha256) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    for path in children(dir)? {
        let meta = fs::symlink_metadata(&path)?;
        let relative = path.strip_prefix(root)?.as_os_str().as_encoded_bytes();
        hash.update((relative.len() as u64).to_le_bytes());
        hash.update(relative);
        hash.update(meta.permissions().mode().to_le_bytes());
        if meta.is_dir() {
            hash.update(b"dir");
            hash_tree(root, &path, hash)?;
        } else if meta.is_file() {
            hash.update(b"file");
            let bytes = fs::read(&path)?;
            hash.update((bytes.len() as u64).to_le_bytes());
            hash.update(bytes);
        } else {
            bail!("unsupported file in Xcode skill: {}", path.display());
        }
    }
    Ok(())
}

fn copy_tree(source: &Path, dest: &Path) -> Result<()> {
    for path in children(source)? {
        let target = dest.join(path.file_name().unwrap());
        let meta = fs::symlink_metadata(&path)?;
        if meta.is_dir() {
            fs::create_dir(&target)?;
            copy_tree(&path, &target)?;
            fs::set_permissions(&target, meta.permissions())?;
        } else if meta.is_file() {
            fs::copy(&path, &target)?;
        } else {
            bail!("unsupported file in Xcode skill: {}", path.display());
        }
    }
    Ok(())
}

/// Xcode serializes its frontmatter dictionary in an unstable order. Sort complete
/// top-level fields, keeping multiline values intact. Leave unfamiliar YAML alone.
fn normalize_frontmatter(text: &str) -> String {
    let Some(rest) = text.strip_prefix("---\n") else {
        return text.into();
    };
    let Some((header, body)) = rest.split_once("\n---\n") else {
        return text.into();
    };
    let mut fields = BTreeMap::<&str, String>::new();
    let mut key = None;
    for line in header.split('\n') {
        if line.starts_with([' ', '\t']) || line.is_empty() {
            let Some(current) = key else {
                return text.into();
            };
            fields.get_mut(current).unwrap().push_str(line);
            fields.get_mut(current).unwrap().push('\n');
        } else {
            let Some((name, value)) = line.split_once(':') else {
                return text.into();
            };
            if name.is_empty()
                || !name
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
                || (!value.is_empty() && !value.starts_with(' '))
                || fields.contains_key(name)
            {
                return text.into();
            }
            fields.insert(name, format!("{line}\n"));
            key = Some(name);
        }
    }
    format!(
        "---\n{}---\n{body}",
        fields.into_values().collect::<String>()
    )
}
