//! App sources: skills exported by an installed app. Exports are
//! machine-generated snapshots, not reproducible git sources.
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};
use sha2::{Digest, Sha256};

use crate::agents::Agent;
use crate::git;
use crate::lock::{CommandSkill, Provenance};
use crate::manifest::{self, CommandOrigin, Manifest, Origin, Skill};
use crate::paths::Paths;
use crate::repositories::Coverage;

/// The alias `[xcode]` declares, and that version 2 locks recorded Xcode skills under.
pub const XCODE: &str = "xcode";

/// Apps skl knows how to export from, like the agents it knows how to link into.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Preset {
    Xcode,
}

impl Preset {
    pub fn named(name: &str) -> Result<Self> {
        match name {
            "xcode" => Ok(Self::Xcode),
            _ => bail!("unknown app `{name}`: use `xcode`, or set `command`"),
        }
    }

    pub fn producer(self, timeout: Option<u64>) -> CommandOrigin {
        let argv = |args: &[&str]| args.iter().map(|a| a.to_string()).collect();
        match self {
            Self::Xcode => CommandOrigin {
                command: argv(&["xcrun", "agent", "skills", "export", "{output}"]),
                version_command: Some(argv(&["xcrun", "xcodebuild", "-version"])),
                timeout: timeout.unwrap_or(30),
            },
        }
    }

    pub fn os(self) -> &'static str {
        match self {
            Self::Xcode => "macos",
        }
    }

    /// Variables the app's commands keep from the environment.
    fn environment(self) -> &'static [&'static str] {
        match self {
            // Selects an Xcode other than the `xcode-select` default.
            Self::Xcode => &["DEVELOPER_DIR"],
        }
    }

    /// Whether the app is installed. `xcrun` exists on every Mac, including those
    /// with only the Command Line Tools, so ask it for Xcode's own build tool.
    fn installed(self) -> Result<bool> {
        match self {
            Self::Xcode => crate::app::succeeds(
                &["xcrun".into(), "--find".into(), "xcodebuild".into()],
                self.environment(),
            ),
        }
    }
}

/// Where a member installs: the source's settings or its own override.
#[derive(Clone)]
pub struct Targets {
    pub agents: Vec<Agent>,
    pub profiles: Vec<String>,
    pub os: Vec<String>,
}

impl Targets {
    pub fn enabled(&self, active: &crate::profile::Active) -> bool {
        manifest::matches_os(&self.os) && active.includes(&self.profiles)
    }
}

/// A declared source whose skills an installed app's command exports.
pub struct App {
    pub producer: CommandOrigin,
    pub preset: Option<Preset>,
    /// None selects every exported skill.
    pub skills: Option<Vec<String>>,
    pub exclude: BTreeSet<String>,
    pub defaults: Targets,
    /// Members with their own settings. An override also selects its member.
    pub overrides: BTreeMap<String, Targets>,
}

impl App {
    pub fn selects(&self, name: &str) -> bool {
        self.overrides.contains_key(name)
            || match &self.skills {
                None => !self.exclude.contains(name),
                Some(names) => names.iter().any(|n| n == name),
            }
    }

    pub fn targets(&self, name: &str) -> &Targets {
        self.overrides.get(name).unwrap_or(&self.defaults)
    }

    /// Whether any member can install here, so the app's export is worth running.
    pub fn enabled(&self, active: &crate::profile::Active) -> bool {
        self.defaults.enabled(active) || self.overrides.values().any(|t| t.enabled(active))
    }

    /// Whether this source selects nothing at all.
    pub fn is_empty(&self) -> bool {
        self.overrides.is_empty() && self.skills.as_ref().is_some_and(Vec::is_empty)
    }

    /// The cached coverage of this exact command, for a wildcard selection.
    pub fn coverage<'a>(
        &self,
        alias: &str,
        coverage: &'a BTreeMap<String, Coverage>,
    ) -> Option<&'a Coverage> {
        coverage
            .get(alias)
            .filter(|c| c.source.is_empty() && c.producer.as_ref() == Some(&self.producer))
    }

    /// Members to install: the list, overrides, and wildcard members with a snapshot.
    /// A wildcard member missing a snapshot waits for an export rather than failing.
    pub fn names(&self, alias: &str, entries: &BTreeMap<String, CommandSkill>) -> Vec<String> {
        let mut names = BTreeSet::new();
        names.extend(self.overrides.keys().cloned());
        names.extend(self.skills.iter().flatten().cloned());
        names.extend(
            entries
                .iter()
                .filter(|(_, e)| e.repository.as_deref() == Some(alias))
                .map(|(n, _)| n.clone()),
        );
        names.into_iter().filter(|n| self.selects(n)).collect()
    }
}

/// Where an app snapshot is cached. Version 2 locks kept Xcode content under `xcode/`.
pub fn stored(paths: &Paths, name: &str, entry: &CommandSkill) -> PathBuf {
    let current = crate::app::skill_dir(paths, name, entry);
    if !current.exists() && entry.repository.as_deref() == Some(XCODE) {
        let legacy = legacy_dir(paths, name, entry);
        if legacy.exists() {
            return legacy;
        }
    }
    current
}

pub fn legacy_dir(paths: &Paths, name: &str, entry: &CommandSkill) -> PathBuf {
    paths
        .store()
        .join("xcode")
        .join(&entry.snapshot.digest)
        .join(name)
}

/// The lock entry for an app source member, if `name` is one.
pub fn member<'a>(lock: &'a crate::lock::Lock, name: &str) -> Option<&'a CommandSkill> {
    lock.commands.get(name).filter(|e| e.repository.is_some())
}

/// Adds the selected members of every app source to the manifest, linking each
/// to its cached snapshot. Sources in `skip` contribute nothing.
pub fn append(
    paths: &Paths,
    manifest: &mut Manifest,
    entries: &BTreeMap<String, CommandSkill>,
    skip: &BTreeSet<String>,
) -> Result<()> {
    let mut members = Vec::new();
    for (alias, app) in &manifest.apps {
        if skip.contains(alias) {
            continue;
        }
        for name in app.names(alias, entries) {
            let dir = entries
                .get(&name)
                .filter(|e| e.repository.as_deref() == Some(alias.as_str()))
                .map(|entry| stored(paths, &name, entry))
                .unwrap_or_else(|| paths.store().join("commands/unavailable").join(&name));
            let targets = app.targets(&name);
            members.push(Skill {
                name,
                origin: Origin::Local(dir),
                agents: targets.agents.clone(),
                profiles: targets.profiles.clone(),
                os: targets.os.clone(),
                group: Some(alias.clone()),
            });
        }
    }
    let mut clashes = Vec::new();
    for (index, skill) in members.iter().enumerate() {
        if let Some(other) = manifest
            .skills
            .iter()
            .chain(&members[..index])
            .find(|s| s.name == skill.name)
        {
            clashes.push(crate::repositories::Clash {
                alias: skill.group.clone().expect("an app member has a source"),
                location: None,
                name: skill.name.clone(),
                other,
            });
        }
    }
    crate::repositories::conflicts(paths, manifest, clashes)?;
    manifest.skills.extend(members);
    Ok(())
}

pub struct Export {
    _stage: tempfile::TempDir,
    version: String,
    root: PathBuf,
    skills: BTreeMap<String, PathBuf>,
}

impl Export {
    /// Runs the app's export, or returns None when the app isn't installed.
    pub fn run(app: &App) -> Result<Option<Self>> {
        let environment = app.preset.map_or(&[][..], Preset::environment);
        let installed = match app.preset {
            Some(preset) => preset.installed()?,
            None => crate::app::executable(&app.producer.command[0])?.is_some(),
        };
        if !installed {
            return Ok(None);
        }
        let stage = tempfile::tempdir().context("creating export staging directory")?;
        let root = stage.path().join("output");
        crate::app::export(&app.producer, stage.path(), &root, environment)?;
        if !fs::symlink_metadata(&root).is_ok_and(|m| m.is_dir()) {
            bail!("the export command must create its {{output}} directory");
        }
        let version = crate::app::version(&app.producer, environment)?;
        let mut skills = BTreeMap::new();
        discover(&root, &mut skills)?;
        Ok(Some(Self {
            _stage: stage,
            version,
            root,
            skills,
        }))
    }

    /// The complete membership this export found.
    pub fn coverage(&self, app: &App) -> Result<Coverage> {
        let mut members = BTreeMap::new();
        for (name, dir) in &self.skills {
            let relative = dir.strip_prefix(&self.root)?.to_string_lossy().into_owned();
            members.insert(name.clone(), relative);
        }
        Ok(Coverage {
            source: String::new(),
            git_ref: None,
            subdir: None,
            producer: Some(app.producer.clone()),
            members,
        })
    }

    /// Snapshots each selected member into the store. A member that fails keeps its
    /// previous entry, and `update` limits refreshing to the named skills. Names
    /// declared elsewhere are rejected before anything is written.
    pub fn install(
        &self,
        paths: &Paths,
        alias: &str,
        app: &App,
        old: &BTreeMap<String, CommandSkill>,
        manifest: &Manifest,
        update: Option<&Vec<String>>,
    ) -> Result<(BTreeMap<String, CommandSkill>, Vec<String>)> {
        let mut names: BTreeSet<String> = app.overrides.keys().cloned().collect();
        match &app.skills {
            None => names.extend(self.skills.keys().filter(|n| app.selects(n)).cloned()),
            Some(list) => names.extend(list.iter().cloned()),
        }
        // A targeted update keeps every other member, even one the app stopped exporting.
        if update.is_some_and(|names| !names.is_empty()) {
            names.extend(old.keys().filter(|n| app.selects(n)).cloned());
        }
        crate::repositories::conflicts(
            paths,
            manifest,
            names
                .iter()
                .filter_map(|name| {
                    let other = manifest.skills.iter().find(|s| &s.name == name)?;
                    Some(crate::repositories::Clash {
                        alias: alias.to_owned(),
                        location: None,
                        name: name.clone(),
                        other,
                    })
                })
                .collect(),
        )?;
        let mut entries = BTreeMap::new();
        let mut errors = Vec::new();
        for name in names {
            let previous = old
                .get(&name)
                .filter(|e| e.repository.as_deref() == Some(alias));
            if update.is_some_and(|names| !names.is_empty() && !names.contains(&name)) {
                if let Some(entry) = previous {
                    entries.insert(name, entry.clone());
                }
                continue;
            }
            let result = (|| -> Result<CommandSkill> {
                let source = self.skills.get(&name).with_context(|| {
                    format!("source `{alias}` no longer exports `{name}`; remove or rename it in the manifest")
                })?;
                let skill_file = source.join("SKILL.md");
                let original = fs::read_to_string(&skill_file)?;
                let normalized = normalize_frontmatter(&original);
                if normalized != original {
                    let permissions = fs::metadata(&skill_file)?.permissions();
                    crate::write_atomic(&skill_file, normalized.as_bytes())?;
                    fs::set_permissions(&skill_file, permissions)?;
                }
                let entry = CommandSkill {
                    producer: app.producer.clone(),
                    repository: Some(alias.to_owned()),
                    snapshot: Provenance {
                        version: self.version.clone(),
                        digest: content_digest(source)?,
                    },
                };
                let dest = crate::app::skill_dir(paths, &name, &entry);
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
                    if let Some(entry) = previous {
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
        .with_context(|| format!("reading export {}", dir.display()))?
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
            bail!("the export contains two skills named `{name}`");
        }
        return Ok(());
    }
    for path in children(dir)? {
        let meta = fs::symlink_metadata(&path)?;
        if meta.is_dir() {
            discover(&path, found)?;
        } else if meta.file_type().is_symlink() {
            bail!("the export contains symlink {}", path.display());
        }
    }
    Ok(())
}

/// Digest a cached export with the same content, type, and mode encoding as installation.
pub fn content_digest(root: &Path) -> Result<String> {
    let mut hash = Sha256::new();
    hash_tree(root, root, &mut hash)?;
    Ok(hash
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
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
            bail!("unsupported file in exported skill: {}", path.display());
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
            bail!("unsupported file in exported skill: {}", path.display());
        }
    }
    Ok(())
}

/// Apps such as Xcode serialize frontmatter in an unstable order. Sort complete
/// top-level fields, keeping multiline values intact. Leave unfamiliar YAML alone.
pub(crate) fn normalize_frontmatter(text: &str) -> String {
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

/// Self-contained exported files, including directory and executable permissions.
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Snapshot {
    nodes: BTreeMap<String, Node>,
}

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Node {
    mode: u32,
    bytes: Option<Vec<u8>>,
}

impl Snapshot {
    pub fn capture(root: &Path) -> Result<Self> {
        if !fs::symlink_metadata(root)?.is_dir() {
            bail!("app snapshot root must be a directory: {}", root.display());
        }
        fn visit(root: &Path, dir: &Path, nodes: &mut BTreeMap<String, Node>) -> Result<()> {
            use std::os::unix::fs::PermissionsExt;
            for path in children(dir)? {
                let meta = fs::symlink_metadata(&path)?;
                if meta.permissions().mode() & 0o7000 != 0 {
                    bail!("special permissions in app snapshot: {}", path.display());
                }
                if !meta.is_file() && !meta.is_dir() {
                    bail!("unsupported file in app snapshot: {}", path.display());
                }
                nodes.insert(
                    path.strip_prefix(root)?
                        .to_str()
                        .context("non-UTF8 snapshot path")?
                        .into(),
                    Node {
                        mode: meta.permissions().mode(),
                        bytes: if meta.is_file() {
                            Some(fs::read(&path)?)
                        } else {
                            None
                        },
                    },
                );
                if meta.is_dir() {
                    visit(root, &path, nodes)?;
                }
            }
            Ok(())
        }
        let mut nodes = BTreeMap::new();
        visit(root, root, &mut nodes)?;
        Ok(Self { nodes })
    }

    fn materialize(&self, root: &Path) -> Result<()> {
        use std::os::unix::fs::PermissionsExt;
        for (name, node) in &self.nodes {
            if node.mode & 0o7000 != 0 {
                bail!("special permissions in app snapshot");
            }
            if name.is_empty() || name.split('/').any(|p| matches!(p, "" | "." | "..")) {
                bail!("invalid app snapshot path `{name}`");
            }
            let path = root.join(name);
            if let Some(bytes) = &node.bytes {
                fs::write(&path, bytes)?;
            } else {
                fs::create_dir(&path)?;
            }
        }
        // Apply restrictive directory permissions only after writing descendants.
        for (name, node) in self.nodes.iter().rev() {
            fs::set_permissions(root.join(name), fs::Permissions::from_mode(node.mode))?;
        }
        if !fs::symlink_metadata(root.join("SKILL.md"))?.is_file() {
            bail!("app snapshot needs a regular SKILL.md");
        }
        Ok(())
    }

    pub fn verify(&self, entry: &Provenance) -> Result<()> {
        let stage = tempfile::tempdir()?;
        self.materialize(stage.path())?;
        if content_digest(stage.path())? != entry.digest {
            bail!("app snapshot digest mismatch");
        }
        Ok(())
    }

    pub fn install(&self, paths: &Paths, name: &str, entry: &CommandSkill) -> Result<()> {
        self.install_at(&crate::app::skill_dir(paths, name, entry), &entry.snapshot)
    }

    pub fn install_at(&self, dest: &Path, entry: &Provenance) -> Result<()> {
        self.verify(entry)?;
        if let Ok(meta) = fs::symlink_metadata(dest) {
            if !meta.is_dir() {
                bail!(
                    "cached app snapshot must be a directory: {}",
                    dest.display()
                );
            }
            if content_digest(dest)? != entry.digest {
                bail!("cached snapshot digest mismatch");
            }
            return Ok(());
        }
        let parent = dest.parent().unwrap();
        fs::create_dir_all(parent)?;
        let stage = tempfile::tempdir_in(parent)?;
        self.materialize(stage.path())?;
        fs::rename(stage.path(), dest)?;
        Ok(())
    }

    pub fn review(&self, previous: Option<&Path>, reverse: bool) -> Result<String> {
        self.diff(previous, reverse, true)
    }

    pub fn diff(&self, previous: Option<&Path>, reverse: bool, patch: bool) -> Result<String> {
        let stage = tempfile::tempdir()?;
        let before = stage.path().join("before");
        let after = stage.path().join("after");
        fs::create_dir(&before)?;
        fs::create_dir(&after)?;
        if let Some(previous) = previous {
            copy_tree(previous, &before)?;
        }
        self.materialize(&after)?;
        let mut command = Command::new("git");
        command.arg("diff");
        command.arg(if patch { "--binary" } else { "--stat" });
        if reverse {
            command.arg("-R");
        }
        let output = command
            .args([
                "--no-index",
                "--no-ext-diff",
                "--no-textconv",
                "--",
                "before",
                "after",
            ])
            .current_dir(stage.path())
            .output()?;
        if !matches!(output.status.code(), Some(0 | 1)) {
            bail!(
                "reviewing app snapshot: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        Ok(String::from_utf8(output.stdout)?)
    }
}

#[cfg(test)]
mod snapshot_tests {
    use super::*;

    #[test]
    fn snapshots_validate_paths_modes_content_and_roots() {
        let entry = Provenance {
            version: "test".into(),
            digest: "0".repeat(64),
        };
        for (name, mode, message) in [
            ("../escape", 0o100644, "invalid app snapshot path"),
            ("/escape", 0o100644, "invalid app snapshot path"),
            ("SKILL.md", 0o104755, "special permissions"),
            ("SKILL.md", 0o100644, "digest mismatch"),
        ] {
            let snapshot = Snapshot {
                nodes: BTreeMap::from([(
                    name.into(),
                    Node {
                        mode,
                        bytes: Some(b"skill".to_vec()),
                    },
                )]),
            };
            assert!(format!("{:#}", snapshot.verify(&entry).unwrap_err()).contains(message));
        }
        let stage = tempfile::tempdir().unwrap();
        fs::create_dir(stage.path().join("real")).unwrap();
        std::os::unix::fs::symlink(stage.path().join("real"), stage.path().join("alias")).unwrap();
        assert!(Snapshot::capture(&stage.path().join("alias")).is_err());
    }
}
