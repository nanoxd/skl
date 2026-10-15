mod agents;
mod git;
mod lock;
mod manifest;
mod paths;
mod plan;
mod profile;
mod sync;
mod xcode;

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use toml_edit::{DocumentMut, InlineTable, Item, Table, value};

use crate::git::Git;
use crate::lock::Lock;
use crate::manifest::Origin;
use crate::paths::Paths;
use crate::profile::Active;

/// Keep agent skills in sync across machines from a manifest and lockfile.
///
/// The manifest (skills.toml) and lock (skills.lock) live in ~/.config/skl, which is
/// meant to be committed to dotfiles. Skills are unpacked into ~/.local/share/skl and
/// linked into each agent's skills directory.
#[derive(Parser)]
#[command(version)]
struct Cli {
    /// Install skills for this profile instead of the machine's own; repeatable
    #[arg(long = "profile", value_name = "NAME", global = true)]
    profiles: Vec<String>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Install, link and prune skills to match the manifest and lock
    Sync {
        /// Show what would change without touching the lock or any links
        #[arg(long)]
        dry_run: bool,
        /// Move anything skl didn't create out of the way instead of skipping it
        #[arg(long)]
        force: bool,
        /// Fail if the lock doesn't already cover the manifest
        #[arg(long)]
        locked: bool,
        /// Print the full diff of skills whose commit changes
        #[arg(long)]
        patch: bool,
    },
    /// Move skills to the newest commit of their ref, then sync
    ///
    /// Each skill that moves prints which of its files changed. Review third-party
    /// changes first with `skl update --dry-run --patch`.
    Update {
        /// Skills to update; all of them when omitted
        #[arg(conflicts_with = "apply_plan")]
        names: Vec<String>,
        /// Save exact commits and full diffs for later review and application
        #[arg(long, value_name = "FILE", conflicts_with_all = ["apply_plan", "dry_run"])]
        plan: Option<PathBuf>,
        /// Apply the exact commits from a saved update plan
        #[arg(long, value_name = "FILE")]
        apply_plan: Option<PathBuf>,
        /// Show what would change without touching the lock or any links
        #[arg(long)]
        dry_run: bool,
        /// Print the full diff of every skill that moves, not just which files changed
        #[arg(long)]
        patch: bool,
    },
    /// Add skills from a git repo (owner/repo or URL) or a local directory, then sync
    Add {
        source: String,
        /// Skill to add from a repo with several; repeatable
        #[arg(short, long = "skill")]
        skills: Vec<String>,
        /// Add every skill in the repo
        #[arg(long, conflicts_with = "skills")]
        all: bool,
        /// Branch, tag or full commit SHA to follow instead of the default branch
        #[arg(long = "ref")]
        git_ref: Option<String>,
        /// Only edit the manifest
        #[arg(long)]
        no_sync: bool,
    },
    /// Remove skills from the manifest and unlink them
    Remove {
        #[arg(required = true)]
        names: Vec<String>,
    },
    /// Show each skill, its locked commit and whether its links are in place, then
    /// anything in the agents' skill directories that skl doesn't manage
    List,
    /// Show or configure the manifest's default agent harnesses
    Agents {
        #[command(subcommand)]
        action: Option<AgentAction>,
    },
    /// Show this machine's profiles, or set them
    Profile {
        #[command(subcommand)]
        action: Option<ProfileAction>,
    },
}

#[derive(Subcommand)]
enum AgentAction {
    /// List supported harnesses and their global skills directories
    List,
    /// Replace the manifest's default agents (per-skill overrides still apply)
    Set {
        #[arg(required = true)]
        names: Vec<String>,
    },
    /// Restore the built-in defaults: claude-code and codex
    Reset,
}

#[derive(Subcommand)]
enum ProfileAction {
    /// Set this machine's profiles, replacing any set before
    Set {
        #[arg(required = true)]
        names: Vec<String>,
    },
    /// Forget this machine's profiles, so only skills without `profiles` install
    Clear,
}

fn main() -> ExitCode {
    // Exit quietly when piped into something like `head` instead of panicking.
    unsafe { libc::signal(libc::SIGPIPE, libc::SIG_DFL) };
    match run(Cli::parse()) {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(err) => {
            eprintln!("error: {err:#}");
            ExitCode::FAILURE
        }
    }
}

fn run(cli: Cli) -> Result<bool> {
    let paths = Paths::from_env()?;
    // Commands that can rewrite shared state take turns.
    let _guard = match cli.command {
        Command::Update { plan: Some(_), .. }
        | Command::Update { dry_run: true, .. }
        | Command::Sync { dry_run: true, .. }
        | Command::List
        | Command::Agents {
            action: None | Some(AgentAction::List),
        } => None,
        _ => Some(lock_run(&paths)?),
    };
    let active = || Active::resolve(&paths, &cli.profiles);
    match cli.command {
        Command::Sync {
            dry_run,
            force,
            locked,
            patch,
        } => sync::run(
            &paths,
            &active()?,
            &sync::Options {
                dry_run,
                force,
                locked,
                patch,
                update: None,
                ..Default::default()
            },
        ),
        Command::Update {
            names,
            dry_run,
            patch,
            plan,
            apply_plan,
        } => sync::run(
            &paths,
            &active()?,
            &sync::Options {
                dry_run,
                patch,
                plan,
                apply_plan,
                update: Some(names),
                ..Default::default()
            },
        ),
        Command::Add {
            source,
            skills,
            all,
            git_ref,
            no_sync,
        } => {
            add(&paths, &source, &skills, all, git_ref)?;
            if no_sync {
                Ok(true)
            } else {
                sync::run(&paths, &active()?, &sync::Options::default())
            }
        }
        Command::Remove { names } => {
            remove(&paths, &names)?;
            sync::run(&paths, &active()?, &sync::Options::default())
        }
        Command::List => list(&paths, &active()?),
        Command::Agents { action } => configure_agents(&paths, action),
        Command::Profile { action } => {
            let changed = action.is_some();
            match action {
                Some(ProfileAction::Set { names }) => profile::set(&paths, &names)?,
                Some(ProfileAction::Clear) => profile::clear(&paths)?,
                None => {}
            }
            let active = Active::resolve(&paths, &cli.profiles)?;
            if active.names.is_empty() {
                println!(
                    "No profiles ({}): only skills without `profiles` install",
                    active.from
                );
            } else {
                println!("{} ({})", active.names.join(", "), active.from);
            }
            if changed {
                println!("Run `skl sync` to apply");
            }
            Ok(true)
        }
    }
}

fn configure_agents(paths: &Paths, action: Option<AgentAction>) -> Result<bool> {
    if matches!(action, Some(AgentAction::List)) {
        for (id, _) in agents::KNOWN {
            let agent = agents::resolve(id, paths)?;
            println!("{id:<20} {}", paths.tilde(&agent.dir));
        }
        return Ok(true);
    }
    let mut doc = read_manifest_doc(paths)?;
    let changed = action.is_some();
    match action {
        Some(AgentAction::Set { names }) => {
            let mut array = toml_edit::Array::new();
            let mut seen = BTreeSet::new();
            for name in names {
                agents::resolve(&name, paths)?;
                if seen.insert(name.clone()) {
                    array.push(name);
                }
            }
            doc["agents"] = value(array);
        }
        Some(AgentAction::Reset) => {
            doc.remove("agents");
        }
        _ => {}
    }
    let manifest = manifest::parse(&doc.to_string(), paths)?;
    if changed {
        write_atomic(&paths.manifest(), doc.to_string().as_bytes())?;
    }
    println!(
        "Default agents: {}",
        manifest
            .agents
            .iter()
            .map(|a| a.id.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    );
    if changed {
        println!("Run `skl sync` to apply");
    }
    Ok(true)
}

fn add(
    paths: &Paths,
    source: &str,
    skills: &[String],
    all: bool,
    git_ref: Option<String>,
) -> Result<()> {
    let local = if source.starts_with('~') {
        paths.expand(source)
    } else {
        std::env::current_dir()?.join(source)
    };
    let entries: Vec<(String, Item)> = if local.is_dir() {
        if all || git_ref.is_some() || skills.len() > 1 {
            bail!("a local directory is one skill: --all, --ref and extra --skill don't apply");
        }
        let dir = std::path::absolute(&local)?;
        let text = fs::read_to_string(dir.join("SKILL.md"))
            .with_context(|| format!("{} has no SKILL.md", paths.tilde(&dir)))?;
        let name = match skills.first() {
            Some(name) => name.clone(),
            None => git::frontmatter_name(&text).unwrap_or_else(|| file_name(&dir)),
        };
        let mut table = InlineTable::new();
        table.insert("path", paths.tilde(&dir).into());
        vec![(name, value(table))]
    } else {
        let url = manifest::source_url(source)?;
        let git = Git::new(paths.repos());
        let commit = git.resolve(&url, git_ref.as_deref())?;
        let repo = git.ensure_commit(&url, &commit)?;
        let candidates = git.candidates(&repo, &commit)?;
        let names: Vec<String> = if all {
            candidates.iter().map(|c| c.name.clone()).collect()
        } else if !skills.is_empty() {
            for name in skills {
                if !candidates
                    .iter()
                    .any(|c| &c.name == name || &c.dir_name == name)
                {
                    let have: Vec<_> = candidates.iter().map(|c| c.name.as_str()).collect();
                    bail!(
                        "{source} has no skill named `{name}`; it has {}",
                        have.join(", ")
                    );
                }
            }
            skills.to_vec()
        } else if let [only] = candidates.as_slice() {
            vec![only.name.clone()]
        } else if candidates.is_empty() {
            bail!("{source} has no SKILL.md");
        } else {
            let have: Vec<_> = candidates.iter().map(|c| c.name.as_str()).collect();
            bail!(
                "{source} has {} skills: pick with --skill or use --all\n  {}",
                have.len(),
                have.join("\n  ")
            );
        };
        names
            .into_iter()
            .map(|name| {
                let item = match &git_ref {
                    None => value(source),
                    Some(r) => {
                        let mut table = InlineTable::new();
                        table.insert("source", source.into());
                        table.insert("ref", r.as_str().into());
                        value(table)
                    }
                };
                (name, item)
            })
            .collect()
    };

    let mut doc = read_manifest_doc(paths)?;
    let table = skills_table(&mut doc)?;
    let mut added = 0;
    for (name, item) in entries {
        manifest::validate_name(&name)?;
        if table.contains_key(&name) {
            println!("= {name} is already in the manifest");
            continue;
        }
        table.insert(&name, item);
        added += 1;
    }
    if added > 0 {
        write_atomic(&paths.manifest(), doc.to_string().as_bytes())?;
    }
    Ok(())
}

fn remove(paths: &Paths, names: &[String]) -> Result<()> {
    let mut doc = read_manifest_doc(paths)?;
    let table = skills_table(&mut doc)?;
    for name in names {
        if table.remove(name).is_none() {
            bail!("`{name}` isn't in the manifest");
        }
    }
    write_atomic(&paths.manifest(), doc.to_string().as_bytes())
}

fn list(paths: &Paths, active: &Active) -> Result<bool> {
    let mut manifest = manifest::load(paths)?;
    let lock = Lock::load(&paths.lock())?;
    xcode::append_skills(paths, &mut manifest, &lock.xcode)?;
    let width = manifest
        .skills
        .iter()
        .map(|s| s.name.len())
        .max()
        .unwrap_or(0);
    if manifest.skills.iter().any(|s| !s.profiles.is_empty()) {
        let names = if active.names.is_empty() {
            "none".to_string()
        } else {
            active.names.join(", ")
        };
        println!("Profiles: {names} ({})\n", active.from);
    }
    let mut healthy = true;
    if let Some(config) = &manifest.xcode
        && config.skills.is_none()
        && !lock.xcode_all_complete
        && config.enabled(active)
    {
        println!("Xcode: all skills selected; run `skl sync` to discover and export them");
        healthy = false;
    }
    if let Some(config) = &manifest.xcode
        && config.skills.is_none()
        && lock.xcode.is_empty()
        && !manifest::matches_os(&config.os)
    {
        println!(
            "Xcode: all skills selected (only on {})",
            config.os.join(", ")
        );
    }
    for skill in &manifest.skills {
        let (origin, version) = match &skill.origin {
            Origin::Git(o) => {
                let version = lock
                    .skills
                    .get(&skill.name)
                    .filter(|l| l.matches(o))
                    .map_or_else(
                        || "not locked".to_string(),
                        |l| {
                            let commit = &l.commit[..7.min(l.commit.len())];
                            o.git_ref
                                .as_ref()
                                .map_or(commit.to_string(), |r| format!("{r} {commit}"))
                        },
                    );
                (o.source.clone(), version)
            }
            Origin::Local(dir) => match lock.xcode.get(&skill.name) {
                Some(entry) if *dir == xcode::skill_dir(paths, &skill.name, entry) => {
                    ("Xcode".into(), entry.version.replace('\n', " / "))
                }
                _ => (paths.tilde(dir), "local".to_string()),
            },
        };
        let status = if !manifest::matches_os(&skill.os) {
            format!("  (only on {})", skill.os.join(", "))
        } else if !active.includes(&skill.profiles) {
            format!("  (only for {})", skill.profiles.join(", "))
        } else {
            let missing: Vec<&str> = skill
                .agents
                .iter()
                .filter(|a| {
                    let link = a.dir.join(&skill.name);
                    !(link.is_symlink() && link.join("SKILL.md").is_file())
                })
                .map(|a| a.id.as_str())
                .collect();
            if missing.is_empty() {
                String::new()
            } else {
                healthy = false;
                format!("  (not linked for {})", missing.join(", "))
            }
        };
        println!("{:<width$}  {version:<18} {origin}{status}", skill.name);
    }

    // Everything else the agents will load: app-installed skills, other tools'
    // installs, and anything left behind.
    let owned = sync::owned_links(paths)?;
    let mut seen = BTreeSet::new();
    let agents = manifest
        .agents
        .iter()
        .chain(manifest.skills.iter().flat_map(|s| &s.agents))
        .filter(|a| seen.insert(a.dir.clone()));
    let mut unmanaged = Vec::new();
    for agent in agents {
        let Ok(entries) = fs::read_dir(&agent.dir) else {
            continue;
        };
        let mut entries: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
        entries.sort();
        for path in entries {
            let name = file_name(&path);
            if name.starts_with('.') {
                continue;
            }
            let target = fs::read_link(&path).ok();
            if target.is_some() && owned.get(&path) == target.as_ref() {
                continue;
            }
            let what = match &target {
                Some(t) => format!("-> {}", paths.tilde(t)),
                None if path.is_dir() => "directory".to_string(),
                None => "file".to_string(),
            };
            unmanaged.push((agent.id.clone(), name, what));
        }
    }
    if !unmanaged.is_empty() {
        println!("\nNot managed by skl:");
        let id_width = unmanaged
            .iter()
            .map(|(id, _, _)| id.len())
            .max()
            .unwrap_or(0);
        let name_width = unmanaged.iter().map(|(_, n, _)| n.len()).max().unwrap_or(0);
        for (id, name, what) in &unmanaged {
            println!("  {id:<id_width$}  {name:<name_width$}  {what}");
        }
    }
    if !healthy {
        eprintln!("Run `skl sync` to fix missing links");
    }
    Ok(healthy)
}

fn read_manifest_doc(paths: &Paths) -> Result<DocumentMut> {
    let path = paths.manifest();
    let text = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(err) => return Err(err).with_context(|| format!("reading {}", paths.tilde(&path))),
    };
    text.parse()
        .with_context(|| format!("invalid manifest {}", paths.tilde(&path)))
}

fn skills_table(doc: &mut DocumentMut) -> Result<&mut Table> {
    doc.entry("skills")
        .or_insert_with(|| Item::Table(Table::new()))
        .as_table_mut()
        .context("`skills` in the manifest must be a table")
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Holds an exclusive lock on the data directory until dropped, waiting for any other
/// skl run to finish first.
fn lock_run(paths: &Paths) -> Result<fs::File> {
    use std::os::fd::AsRawFd;

    fs::create_dir_all(&paths.data)?;
    let path = paths.data.join("run.lock");
    let file = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&path)
        .with_context(|| format!("opening {}", path.display()))?;
    let fd = file.as_raw_fd();
    if unsafe { libc::flock(fd, libc::LOCK_EX | libc::LOCK_NB) } != 0 {
        eprintln!("Waiting for another skl run to finish…");
        if unsafe { libc::flock(fd, libc::LOCK_EX) } != 0 {
            return Err(std::io::Error::last_os_error())
                .with_context(|| format!("locking {}", path.display()));
        }
    }
    Ok(file)
}

/// Writes through a temporary file so an interrupted run never leaves half a file.
/// A symlinked file, such as a manifest linked from a dotfiles checkout, is written
/// at its target so the link survives.
pub fn write_atomic(path: &Path, contents: &[u8]) -> Result<()> {
    let mut path = path.to_path_buf();
    for _ in 0..32 {
        let Ok(target) = fs::read_link(&path) else {
            break;
        };
        path = path.parent().context("path has no parent")?.join(target);
    }
    let path = path.as_path();
    let dir = path.parent().context("path has no parent")?;
    fs::create_dir_all(dir)?;
    let tmp: PathBuf = dir.join(format!(".{}.tmp-{}", file_name(path), std::process::id()));
    fs::write(&tmp, contents).with_context(|| format!("writing {}", tmp.display()))?;
    fs::rename(&tmp, path).with_context(|| format!("writing {}", path.display()))
}
