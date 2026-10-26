mod adopt;
mod agents;
mod app;
mod discover;
mod doctor;
mod export;
mod git;
mod info;
mod interactive;
mod json;
mod lock;
mod manifest;
mod paths;
mod plan;
mod plugin;
mod plugins;
mod profile;
mod repositories;
mod search;
mod source;
mod sync;
mod verify;

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, Result, bail};
use clap::{Args, CommandFactory, Parser, Subcommand};
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
    /// Use skills.toml and skills.lock in this project, with project agent directories
    #[arg(long, value_name = "PATH", global = true)]
    project: Option<PathBuf>,

    /// Install skills for this profile instead of the machine's own; repeatable
    #[arg(long = "profile", value_name = "NAME", global = true)]
    profiles: Vec<String>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Generate shell completions on stdout without reading configuration
    Completions {
        #[arg(value_enum)]
        shell: clap_complete::Shell,
    },
    /// Search the skills.sh catalog without installing anything
    Search {
        #[arg(required = true, num_args = 1..)]
        query: Vec<String>,
        /// Restrict results to a GitHub owner
        #[arg(long)]
        owner: Option<String>,
        /// Maximum results (1–100)
        #[arg(long, default_value_t = 20)]
        limit: u16,
        #[arg(long)]
        json: bool,
    },
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
        /// Choose skills and agents with numbered terminal prompts
        #[arg(long)]
        interactive: bool,
        #[command(flatten)]
        options: SkillOptions,
        /// Skill to add from a repository or local collection; repeatable
        #[arg(short, long = "skill")]
        skills: Vec<String>,
        /// Add every skill in the repository or local collection
        #[arg(long, conflicts_with = "skills")]
        all: bool,
        /// Branch, tag or full commit SHA to follow instead of the default branch
        #[arg(long = "ref")]
        git_ref: Option<String>,
        /// Directory containing the skill, or a collection of skills, within the repository
        #[arg(long)]
        subdir: Option<String>,
        /// Install this plugin, with its agents, commands and hooks, instead of skills
        #[arg(long, value_name = "NAME", conflicts_with_all = ["skills", "all", "interactive"])]
        plugin: Option<String>,
        /// Manifest name for `--plugin`, or for the source `--all` declares for a collection
        #[arg(long, value_name = "NAME")]
        alias: Option<String>,
        /// Only edit the manifest
        #[arg(long)]
        no_sync: bool,
    },
    /// Snapshot an unmanaged installed skill into portable local configuration
    Adopt {
        /// Installed directory name (as shown by `skl list`)
        name: String,
        /// Agent name or skills directory containing the installed skill
        #[arg(long)]
        from: String,
        #[command(flatten)]
        options: SkillOptions,
        /// Copy and register only; leave installation for a later sync
        #[arg(long)]
        no_sync: bool,
        /// Validate and preview without writing anything
        #[arg(long)]
        dry_run: bool,
    },
    /// Browse skills in a git repository without installing them
    Discover {
        /// Repository shorthand (owner/repo) or git URL
        source: String,
        /// Branch, tag or full commit SHA instead of the default branch
        #[arg(long = "ref")]
        git_ref: Option<String>,
    },
    /// Edit per-skill settings, then sync (use --no-sync to avoid fetching)
    Configure {
        #[arg(required = true)]
        names: Vec<String>,
        #[command(flatten)]
        options: SkillOptions,
        /// Inherit the default agents: the source's, or the manifest's
        #[arg(long, conflicts_with = "agents")]
        default_agents: bool,
        /// Remove operating-system restrictions
        #[arg(long, conflicts_with = "os")]
        all_os: bool,
        /// Remove profile restrictions
        #[arg(long, conflicts_with = "only_profiles")]
        all_profiles: bool,
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
    List {
        /// Emit one versioned JSON document
        #[arg(long)]
        json: bool,
    },
    /// Inspect one skill or plugin without changing files or contacting its source
    Info {
        name: String,
        /// Emit one versioned JSON document
        #[arg(long)]
        json: bool,
    },
    /// Diagnose local installation problems without changing files or using the network
    Doctor {
        /// Emit one versioned JSON document
        #[arg(long)]
        json: bool,
    },
    /// Verify cached content against pinned Git commits and Xcode digests, offline
    Verify {
        /// Skills to verify; all selected skills when omitted
        names: Vec<String>,
        /// Emit one versioned JSON document
        #[arg(long)]
        json: bool,
    },
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

#[derive(Args, Default)]
struct SkillOptions {
    /// Replace this skill's agent destinations; repeatable (name, ~/path or /absolute/path)
    #[arg(long = "agent", value_name = "AGENT")]
    agents: Vec<String>,
    /// Restrict this skill to operating systems; repeatable
    #[arg(long, value_parser = ["macos", "linux"])]
    os: Vec<String>,
    /// Restrict this skill to profiles; repeatable (distinct from global --profile)
    #[arg(long = "only-profile", value_name = "NAME")]
    only_profiles: Vec<String>,
}

impl SkillOptions {
    fn validate(&self, paths: &Paths) -> Result<()> {
        for agent in &self.agents {
            agents::resolve(agent, paths)?;
        }
        for profile in &self.only_profiles {
            profile::validate(profile)?;
        }
        Ok(())
    }

    /// The settings a source's member needs beyond the declaration's defaults.
    fn beyond(&self, declaration: &repositories::Declaration) -> Self {
        let keep = |key: &str, values: &Vec<String>| {
            let inherited = declaration
                .defaults
                .get(key)
                .is_some_and(|defaults| defaults.iter().eq(values.iter().collect::<BTreeSet<_>>()));
            if inherited {
                Vec::new()
            } else {
                values.clone()
            }
        };
        Self {
            agents: keep("agents", &self.agents),
            os: keep("os", &self.os),
            only_profiles: keep("profiles", &self.only_profiles),
        }
    }

    /// The declaration's defaults a member keeps because these options don't replace them.
    fn inherited(&self, declaration: &repositories::Declaration) -> Vec<String> {
        [
            ("agents", &self.agents),
            ("os", &self.os),
            ("profiles", &self.only_profiles),
        ]
        .into_iter()
        .filter(|(_, own)| own.is_empty())
        .filter_map(|(key, _)| {
            let values: Vec<_> = declaration
                .defaults
                .get(key)?
                .iter()
                .map(|v| format!("{v:?}"))
                .collect();
            Some(format!("{key} = [{}]", values.join(", ")))
        })
        .collect()
    }

    fn apply(&self, item: &mut Item, resets: [bool; 3]) -> Result<()> {
        let fields = [
            ("agents", &self.agents),
            ("os", &self.os),
            ("profiles", &self.only_profiles),
        ];
        // Resetting an absent override should not expand shorthand entries.
        if item.is_str() && fields.iter().all(|(_, values)| values.is_empty()) {
            return Ok(());
        }
        if let Some(source) = item.as_str() {
            let mut table = InlineTable::new();
            table.insert("source", source.into());
            let mut replacement = toml_edit::Value::InlineTable(table);
            *replacement.decor_mut() = item.as_value().unwrap().decor().clone();
            *item = Item::Value(replacement);
        }
        let table = item
            .as_table_like_mut()
            .context("expected a skill source or table")?;
        for ((key, values), reset) in fields.into_iter().zip(resets) {
            if reset {
                table.remove(key);
            } else if !values.is_empty() {
                let mut array = toml_edit::Array::new();
                let mut seen = BTreeSet::new();
                for entry in values {
                    if seen.insert(entry) {
                        array.push(entry.as_str());
                    }
                }
                let mut replacement = toml_edit::Value::Array(array);
                if let Some(old) = table.get(key).and_then(Item::as_value) {
                    *replacement.decor_mut() = old.decor().clone();
                }
                table.insert(key, Item::Value(replacement));
            }
        }
        Ok(())
    }
}

#[derive(Subcommand)]
enum AgentAction {
    /// List supported harnesses and their global skills directories
    List,
    /// Show harnesses with recognizable configuration directories (read-only)
    Detect,
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
    let cli = Cli::parse();
    let json_command = match &cli.command {
        Command::List { json: true } => Some("list"),
        Command::Search { json: true, .. } => Some("search"),
        Command::Info { json: true, .. } => Some("info"),
        Command::Doctor { json: true } => Some("doctor"),
        Command::Verify { json: true, .. } => Some("verify"),
        _ => None,
    };
    match run(cli) {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(err) => {
            if let Some(command) = json_command {
                json::error(command, &err);
            } else {
                eprintln!("error: {err:#}");
            }
            ExitCode::FAILURE
        }
    }
}

fn run(cli: Cli) -> Result<bool> {
    if let Command::Completions { shell } = &cli.command {
        clap_complete::generate(*shell, &mut Cli::command(), "skl", &mut std::io::stdout());
        return Ok(true);
    }
    if let Command::Search {
        query,
        owner,
        limit,
        json,
    } = &cli.command
    {
        return search::run(query, owner.as_deref(), *limit, *json);
    }
    let paths = match &cli.project {
        Some(root) => Paths::for_project(root)?,
        None => Paths::from_env()?,
    };
    // Commands that can rewrite shared state take turns.
    let _guard = match cli.command {
        Command::Update { plan: Some(_), .. }
        | Command::Update { dry_run: true, .. }
        | Command::Sync { dry_run: true, .. }
        | Command::Adopt { dry_run: true, .. }
        | Command::List { .. }
        | Command::Info { .. }
        | Command::Verify { .. }
        | Command::Doctor { .. }
        | Command::Discover { .. }
        | Command::Add {
            interactive: true, ..
        }
        | Command::Agents {
            action: None | Some(AgentAction::List | AgentAction::Detect),
        } => None,
        _ => Some(lock_run(&paths)?),
    };
    let active = || Active::resolve(&paths, &cli.profiles);
    match cli.command {
        Command::Search { .. } | Command::Completions { .. } => {
            unreachable!("command handled before resolving machine paths")
        }
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
                run_commands: true,
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
                run_commands: true,
                update: Some(names),
                ..Default::default()
            },
        ),
        Command::Add {
            source,
            interactive,
            mut options,
            mut skills,
            all,
            git_ref,
            subdir,
            plugin,
            alias,
            no_sync,
        } => {
            let source = source::Source::parse(&source, git_ref.as_deref(), subdir.as_deref())?;
            if let Some(plugin) = plugin {
                add_plugin(&paths, &source, &plugin, alias.as_deref(), &options)?;
                return if no_sync {
                    Ok(true)
                } else {
                    sync::run(&paths, &active()?, &sync::Options::default())
                };
            }
            let mut selected_collection = false;
            let _interactive_guard = if interactive {
                selected_collection =
                    interactive::select(&paths, &source, &mut skills, all, &mut options)?;
                Some(lock_run(&paths)?)
            } else {
                None
            };
            add(
                &paths,
                &source,
                &skills,
                all,
                &options,
                selected_collection,
                alias.as_deref(),
            )?;
            if no_sync {
                Ok(true)
            } else {
                sync::run(&paths, &active()?, &sync::Options::default())
            }
        }
        Command::Adopt {
            name,
            from,
            options,
            no_sync,
            dry_run,
        } => {
            adopt::run(&paths, &name, &from, &options, dry_run)?;
            if no_sync || dry_run {
                Ok(true)
            } else {
                sync::run(&paths, &active()?, &sync::Options::default())
            }
        }
        Command::Configure {
            names,
            options,
            default_agents,
            all_os,
            all_profiles,
            no_sync,
        } => {
            configure_skills(
                &paths,
                &names,
                &options,
                [default_agents, all_os, all_profiles],
            )?;
            if no_sync {
                Ok(true)
            } else {
                sync::run(&paths, &active()?, &sync::Options::default())
            }
        }
        Command::Discover { source, git_ref } => discover::run(&source, git_ref.as_deref()),
        Command::Remove { names } => {
            remove(&paths, &names)?;
            sync::run(&paths, &active()?, &sync::Options::default())
        }
        Command::List { json: true } => json::list(&paths, &active()?),
        Command::List { json: false } => list(&paths, &active()?),
        Command::Info { name, json: true } => json::info(&paths, &active()?, &name),
        Command::Info { name, json: false } => {
            let active = active()?;
            match info::plugin(&paths, &active, &name)? {
                Some(report) => report.print(&paths),
                None => info::collect(&paths, &active, &name)?.print(&paths),
            }
            Ok(true)
        }
        Command::Verify { names, json } => verify::run(&paths, &active()?, &names, json),
        Command::Doctor { json: true } => json::doctor(&paths, &active()?),
        Command::Doctor { json: false } => doctor::run(&paths, &active()?),
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
    if matches!(action, Some(AgentAction::Detect)) {
        let detected = agents::detect(paths)?;
        if detected.is_empty() {
            println!("No recognized agent configuration directories found.");
        }
        for agent in detected {
            println!(
                "{:<20} {} (configuration found)",
                agent.id,
                paths.tilde(&agent.dir)
            );
        }
        return Ok(true);
    }
    if matches!(action, Some(AgentAction::List)) {
        for (id, _) in agents::known(paths) {
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
    input: &source::Source,
    skills: &[String],
    all: bool,
    options: &SkillOptions,
    // A picker-selected child must not become an alias for the root SKILL.md.
    selected_collection: bool,
    alias: Option<&str>,
) -> Result<()> {
    options.validate(paths)?;
    let source = input.repository.as_str();
    let git_ref = &input.git_ref;
    let subdir = &input.subdir;
    let selected;
    let skills = if let Some(skill) = &input.skill {
        if all || (!skills.is_empty() && skills != [skill.clone()]) {
            bail!("@skill conflicts with --all or --skill");
        }
        selected = vec![skill.clone()];
        &selected[..]
    } else {
        skills
    };
    let local = if source.starts_with('~') {
        paths.expand(source)
    } else {
        std::env::current_dir()?.join(source)
    };
    let mut doc = read_manifest_doc(paths)?;
    let declarations = repositories::declarations(&doc);
    // A directory that a declared source names is that Git repository.
    let declared = if local.is_dir() {
        let dir = std::path::absolute(&local)?;
        declarations
            .iter()
            .filter_map(|d| d.source.as_deref())
            .find(|s| s.starts_with('/') && std::path::Path::new(s) == dir)
    } else {
        None
    };
    let source = declared.unwrap_or(source);
    let mut plugins = Vec::new();
    let entries: Vec<(String, Entry)> = if local.is_dir() && declared.is_none() {
        if git_ref.is_some() || subdir.is_some() {
            bail!("--ref and --subdir do not apply to local directories");
        }
        let dir = if paths.project.is_some() {
            local.canonicalize()?
        } else {
            std::path::absolute(&local)?
        };
        let selected = if dir.join("SKILL.md").is_file()
            && !all
            && skills.len() <= 1
            && !selected_collection
        {
            // Preserve the existing single-skill workflow, including --skill renaming.
            let text = fs::read_to_string(dir.join("SKILL.md"))
                .with_context(|| format!("cannot read {}", paths.tilde(&dir.join("SKILL.md"))))?;
            let name = skills
                .first()
                .cloned()
                .unwrap_or_else(|| git::frontmatter_name(&text).unwrap_or_else(|| file_name(&dir)));
            vec![(name, dir.clone())]
        } else {
            let candidates = git::local_candidates(&dir)?;
            let names = if all {
                candidates
                    .iter()
                    .map(|c| c.name.clone())
                    .collect::<Vec<_>>()
            } else if !skills.is_empty() {
                skills.to_vec()
            } else if let [only] = candidates.as_slice() {
                vec![only.name.clone()]
            } else if candidates.is_empty() {
                bail!("{} has no regular SKILL.md", paths.tilde(&dir));
            } else {
                bail!(
                    "{} has {} skills: pick with --skill or use --all",
                    paths.tilde(&dir),
                    candidates.len()
                );
            };
            if candidates.is_empty() {
                bail!("{} has no regular SKILL.md", paths.tilde(&dir));
            }
            let mut selected = Vec::new();
            for name in names {
                let by_name = candidates
                    .iter()
                    .filter(|c| c.name == name)
                    .collect::<Vec<_>>();
                let matches = if by_name.is_empty() {
                    candidates
                        .iter()
                        .filter(|c| c.dir_name == name)
                        .collect::<Vec<_>>()
                } else {
                    by_name
                };
                match matches.as_slice() {
                    [one] => selected.push((name, dir.join(&one.subdir))),
                    [] => bail!("{} has no skill named `{name}`", paths.tilde(&dir)),
                    _ => bail!(
                        "several local skills match `{name}` ({}): add each directory separately with a unique --skill name",
                        matches
                            .iter()
                            .map(|c| c.subdir.as_str())
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                }
            }
            selected
        };
        selected
            .into_iter()
            .map(|(name, dir)| {
                let mut table = InlineTable::new();
                table.insert("path", paths.manifest_path(&dir).into());
                (name, Entry::Own(value(table)))
            })
            .collect()
    } else {
        let url = manifest::source_url(source)?;
        let git = Git::new(paths.repos());
        let group = join_group(
            &declarations,
            &git,
            &url,
            source,
            git_ref.as_deref(),
            subdir.as_deref(),
        )?;
        // Without a ref of its own, a member is checked at the ref it will inherit.
        let commit = match group.and_then(|(declaration, _)| declaration.git_ref.as_deref()) {
            Some(inherited) if git_ref.is_none() => git.resolve(&url, Some(inherited))?,
            _ => input.resolve(&git, &url)?,
        };
        let repo = git.ensure_commit(&url, &commit)?;
        // A directory holds one skill or, without a SKILL.md of its own, a collection.
        // Without one, a declared source's `subdir` limits the lookup.
        let scope = group.and_then(|(declaration, _)| declaration.subdir.as_deref());
        let (candidates, collection) = match subdir {
            Some(dir) => git.lookup(&repo, &commit, Some(dir))?,
            None => (git.candidates_under(&repo, &commit, scope)?, false),
        };
        let under = match subdir.as_deref().or(scope) {
            Some(dir) if dir != "." => format!(" under `{dir}`"),
            _ => String::new(),
        };
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
                        "{source} has no skill named `{name}`{under}; it has {}",
                        have.join(", ")
                    );
                }
            }
            skills.to_vec()
        } else if let [only] = candidates.as_slice() {
            vec![only.name.clone()]
        } else if candidates.is_empty() {
            bail!("{source} has no SKILL.md{under}");
        } else {
            let have: Vec<_> = candidates.iter().map(|c| c.name.as_str()).collect();
            bail!(
                "{source} has {} skills{under}: pick with --skill or use --all\n  {}",
                have.len(),
                have.join("\n  ")
            );
        };
        // Reject ambiguous names before editing the manifest, including --no-sync.
        let mut chosen = Vec::with_capacity(names.len());
        for name in &names {
            let by_name: Vec<_> = candidates.iter().filter(|c| &c.name == name).collect();
            let matches = if by_name.is_empty() {
                candidates
                    .iter()
                    .filter(|c| &c.dir_name == name)
                    .collect::<Vec<_>>()
            } else {
                by_name
            };
            if matches.len() > 1 {
                let dirs: Vec<_> = matches.iter().map(|c| c.subdir.as_str()).collect();
                bail!(
                    "several skills in {source}{under} match `{name}` ({}): choose one collection with --subdir {} or its tree URL",
                    dirs.join(", "),
                    git::parents(&dirs).join(" or --subdir ")
                );
            }
            chosen.push(matches.first().copied());
        }
        let files = git.files(&repo, &commit)?;
        let dirs: Vec<&str> = chosen.iter().flatten().map(|c| c.subdir.as_str()).collect();
        plugins = plugin::containing(&git, &repo, &commit, &files, &dirs);
        // A collection inside one plugin takes the plugin's name.
        let plugin_name = match plugins.as_slice() {
            [only] => Some(only.name.clone()),
            _ => None,
        };
        plugins.retain(plugin::Plugin::has_more);
        // `--all` on a collection declares one source that keeps tracking it.
        if let (true, true, None, Some(dir)) = (all, collection, group, subdir) {
            add_source(
                paths,
                &mut doc,
                source,
                git_ref.as_deref(),
                dir,
                &names,
                options,
                alias.or(plugin_name.as_deref()),
            )?;
            for plugin in &plugins {
                plugin.warn(source);
            }
            return Ok(());
        }
        // A member found by name elsewhere in its source keeps its directory.
        let everywhere = match (group, subdir) {
            (Some(_), Some(_)) => git.candidates_under(&repo, &commit, scope)?,
            _ => Vec::new(),
        };
        let all = if subdir.is_some() {
            &everywhere
        } else {
            &candidates
        };
        names
            .into_iter()
            .zip(chosen)
            .map(|(name, candidate)| {
                // A collection records each skill's own directory.
                let directory = subdir.as_ref().map(|dir| match candidate {
                    Some(c) if collection => c.subdir.clone(),
                    _ => dir.clone(),
                });
                if let Some((declaration, own_ref)) = group {
                    let mut table = InlineTable::new();
                    table.insert("source", declaration.alias.as_str().into());
                    if let Some(r) = git_ref.as_deref().filter(|_| own_ref) {
                        table.insert("ref", r.into());
                    }
                    if let Some(dir) = &directory {
                        let exact = if dir == "." { "" } else { dir.as_str() };
                        if !matches!(git::matching(&everywhere, &name)[..], [one] if one.subdir == exact)
                        {
                            table.insert("subdir", dir.as_str().into());
                        }
                    }
                    let options = options.beyond(declaration);
                    let entry = Entry::Member {
                        alias: declaration.alias.clone(),
                        item: value(table),
                        inherits: options.inherited(declaration),
                        options,
                        discovered: all.iter().any(|c| c.name == name),
                    };
                    return (name, entry);
                }
                let item = match (git_ref, &directory) {
                    (None, None) => value(source),
                    (reference, directory) => {
                        let mut table = InlineTable::new();
                        table.insert("source", source.into());
                        if let Some(r) = reference {
                            table.insert("ref", r.as_str().into());
                        }
                        if let Some(d) = directory {
                            table.insert("subdir", d.as_str().into());
                        }
                        value(table)
                    }
                };
                (name, Entry::Own(item))
            })
            .collect()
    };

    // Only `--all` on a collection directory declares a source, and returns above.
    if alias.is_some() {
        bail!(
            "--alias names a plugin, or the source that --all declares for a collection directory, a folder without a SKILL.md of its own"
        );
    }
    let coverage = Lock::load(&paths.lock())?.repositories;
    let mut added = 0;
    for (name, entry) in entries {
        manifest::validate_name(&name)?;
        // Read the edited document so a name repeated in one command joins once.
        if doc.get("skills").and_then(|s| s.get(&name)).is_some()
            || repositories::declarations(&doc)
                .iter()
                .any(|d| d.selects(&name, &coverage))
        {
            if !options.agents.is_empty()
                || !options.os.is_empty()
                || !options.only_profiles.is_empty()
            {
                println!(
                    "= {name} is already in the manifest; use `skl configure {name}` to change settings"
                );
            } else {
                println!("= {name} is already in the manifest");
            }
            continue;
        }
        match entry {
            Entry::Own(mut item) => {
                options.apply(&mut item, [false; 3])?;
                insert_entry(skills_table(&mut doc)?, &name, item);
            }
            Entry::Member {
                alias,
                mut item,
                options,
                discovered,
                inherits,
            } => {
                options.apply(&mut item, [false; 3])?;
                let overrides = item.as_inline_table().is_some_and(|t| t.len() > 1);
                let group = repositories::declaration_mut(&mut doc, &alias)
                    .context("source declarations must be tables")?;
                let wildcard = group.get("skills").and_then(Item::as_str) == Some("*");
                let restored = wildcard
                    && group
                        .get_mut("exclude")
                        .and_then(Item::as_array_mut)
                        .is_some_and(|list| repositories::remove_name(list, &name));
                // Each name appears once: in the source's list while it inherits
                // everything, otherwise as its own `source` entry. A wildcard
                // selects discovered names, never directory names.
                let list = group.get_mut("skills").and_then(Item::as_array_mut);
                if overrides {
                    insert_entry(skills_table(&mut doc)?, &name, item);
                } else if (wildcard && !discovered) || (!wildcard && list.is_none()) {
                    // A bare reference takes the string form, like `name = "owner/repo"`.
                    insert_entry(skills_table(&mut doc)?, &name, value(alias.as_str()));
                } else if let Some(list) = list {
                    repositories::insert_name(list, &name);
                } else if !restored {
                    println!("= {name} is already selected by source `{alias}`");
                    continue;
                }
                // Inherited restrictions can narrow where a skill installs, so show them.
                if inherits.is_empty() {
                    println!("Added {name} to source `{alias}`");
                } else {
                    println!(
                        "Added {name} to source `{alias}`, inheriting {}",
                        inherits.join(", ")
                    );
                }
            }
        }
        added += 1;
    }
    manifest::parse(&doc.to_string(), paths)?;
    if added > 0 {
        write_atomic(&paths.manifest(), doc.to_string().as_bytes())?;
        for plugin in &plugins {
            plugin.warn(source);
        }
    }
    Ok(())
}

/// How `add` records one skill.
enum Entry {
    /// A `[skills]` entry naming its own source or path.
    Own(Item),
    /// A skill from a declared source, like `cargo add` inheriting a workspace
    /// dependency. `item` is the `source` reference plus anything it overrides.
    Member {
        alias: String,
        item: Item,
        options: SkillOptions,
        /// Whether discovery lists the skill under this name rather than another.
        discovered: bool,
        /// The declaration's settings the skill takes on, such as `profiles = ["work"]`.
        inherits: Vec<String>,
    },
}

/// The source declaration a Git skill joins and whether it needs its own `ref`.
/// Without `--ref`, the skill inherits the declaration's ref. A declaration with a
/// `subdir` only takes skills from inside it.
fn join_group<'a>(
    declarations: &'a [repositories::Declaration],
    git: &Git,
    url: &str,
    source: &str,
    git_ref: Option<&str>,
    subdir: Option<&str>,
) -> Result<Option<(&'a repositories::Declaration, bool)>> {
    let matching: Vec<_> = declarations
        .iter()
        .filter(|d| {
            d.source
                .as_deref()
                .is_some_and(|declared| repositories::same_repository(declared, source))
                && subdir.is_none_or(|dir| git::within(dir, d.subdir.as_deref()))
        })
        .collect();
    let Some(&first) = matching.first() else {
        return Ok(None);
    };
    let follows_default = matching.iter().copied().find(|d| d.git_ref.is_none());
    let Some(wanted) = git_ref else {
        return Ok(Some((follows_default.unwrap_or(first), false)));
    };
    if let Some(&same) = matching
        .iter()
        .find(|d| d.git_ref.as_deref() == Some(wanted))
    {
        return Ok(Some((same, false)));
    }
    // A tree URL names the default branch that a declaration without a ref follows.
    if let Some(declaration) = follows_default
        && git.default_branch(url)?.as_deref() == Some(wanted)
    {
        return Ok(Some((declaration, false)));
    }
    Ok(Some((first, true)))
}

/// Declares a source that selects every skill in a collection directory, so the
/// manifest keeps tracking it. Names the manifest already has stay where they are
/// and are excluded here.
#[allow(clippy::too_many_arguments)]
fn add_source(
    paths: &Paths,
    doc: &mut DocumentMut,
    source: &str,
    git_ref: Option<&str>,
    dir: &str,
    names: &[String],
    options: &SkillOptions,
    alias: Option<&str>,
) -> Result<()> {
    let declarations = repositories::declarations(doc);
    let alias = match alias {
        Some(alias) => alias.to_owned(),
        // The collection's own name, skipping generic folders like `skills`.
        None => dir
            .rsplit('/')
            .find(|part| *part != "skills")
            .unwrap_or(dir)
            .to_owned(),
    };
    manifest::validate_name(&alias).context("name the source with --alias NAME")?;
    if declarations.iter().any(|d| d.alias == alias) {
        bail!("source `{alias}` is already declared; name this one with --alias NAME");
    }
    let coverage = Lock::load(&paths.lock())?.repositories;
    let taken: Vec<&String> = names
        .iter()
        .filter(|name| {
            doc.get("skills").and_then(|s| s.get(name)).is_some()
                || declarations.iter().any(|d| d.selects(name, &coverage))
        })
        .collect();
    let mut table = Table::new();
    table.insert("source", value(source));
    if let Some(reference) = git_ref {
        table.insert("ref", value(reference));
    }
    table.insert("subdir", value(dir));
    table.insert("skills", value("*"));
    if !taken.is_empty() {
        table.insert(
            "exclude",
            value(
                taken
                    .iter()
                    .map(|n| n.as_str())
                    .collect::<toml_edit::Array>(),
            ),
        );
    }
    for (key, values) in [
        ("agents", &options.agents),
        ("os", &options.os),
        ("profiles", &options.only_profiles),
    ] {
        let mut seen = BTreeSet::new();
        let unique: toml_edit::Array = values
            .iter()
            .filter(|v| seen.insert(*v))
            .map(String::as_str)
            .collect();
        if !unique.is_empty() {
            table.insert(key, value(unique));
        }
    }
    insert_declaration(doc, "sources", &alias, table)?;
    manifest::parse(&doc.to_string(), paths)?;
    write_atomic(&paths.manifest(), doc.to_string().as_bytes())?;
    let count = names.len() - taken.len();
    println!("Added source `{alias}` with every skill in {dir} ({count} now, and any added later)");
    if !taken.is_empty() {
        let taken: Vec<&str> = taken.iter().map(|n| n.as_str()).collect();
        println!(
            "= excluded {} from `{alias}`: already in the manifest",
            taken.join(", ")
        );
    }
    Ok(())
}

/// Adds `[section.name]` above `[skills]`, as the documentation writes declarations.
fn insert_declaration(
    doc: &mut DocumentMut,
    section: &str,
    name: &str,
    mut table: Table,
) -> Result<()> {
    if let Some(skills) = doc.get_mut("skills").and_then(Item::as_table_mut)
        && let Some(position) = skills.position()
    {
        table.set_position(Some(position - 1));
        let prefix = skills
            .decor()
            .prefix()
            .and_then(|p| p.as_str())
            .unwrap_or("");
        if !prefix.starts_with('\n') {
            let prefix = format!("\n{prefix}");
            skills.decor_mut().set_prefix(prefix);
        }
    }
    let parent = doc.entry(section).or_insert_with(|| {
        let mut parent = Table::new();
        parent.set_implicit(true);
        Item::Table(parent)
    });
    parent
        .as_table_mut()
        .with_context(|| format!("`{section}` in the manifest must be a table"))?
        .insert(name, Item::Table(table));
    Ok(())
}

/// Adds `[plugins.<name>]` once every agent can find the plugin in the repository.
fn add_plugin(
    paths: &Paths,
    input: &source::Source,
    plugin: &str,
    alias: Option<&str>,
    options: &SkillOptions,
) -> Result<()> {
    options.validate(paths)?;
    if input.skill.is_some() {
        bail!("@skill conflicts with --plugin");
    }
    let name = alias.unwrap_or(plugin);
    manifest::validate_name(name).context("name the plugin with --alias NAME")?;
    let mut doc = read_manifest_doc(paths)?;
    if doc.get("plugins").and_then(|p| p.get(name)).is_some() {
        println!("= plugin {name} is already in the manifest");
        return Ok(());
    }
    let mut table = Table::new();
    table.insert("source", value(input.repository.as_str()));
    if let Some(reference) = &input.git_ref {
        table.insert("ref", value(reference.as_str()));
    }
    if plugin != name {
        table.insert("plugin", value(plugin));
    }
    if let Some(dir) = &input.subdir {
        table.insert("subdir", value(dir.as_str()));
    }
    for (key, values) in [
        ("agents", &options.agents),
        ("os", &options.os),
        ("profiles", &options.only_profiles),
    ] {
        let mut seen = BTreeSet::new();
        let unique: toml_edit::Array = values
            .iter()
            .filter(|v| seen.insert(*v))
            .map(String::as_str)
            .collect();
        if !unique.is_empty() {
            table.insert(key, value(unique));
        }
    }
    // Check the entry the way sync will read it before writing anything.
    let manifest = manifest::parse(&doc.to_string(), paths)?;
    let parsed = plugins::parse_one(
        name,
        toml::Value::Table(toml::from_str(&table.to_string())?),
        &manifest.agents,
        paths,
    )
    .with_context(|| format!("plugin `{name}`"))?;
    let folders = plugins::check(paths, &parsed)?;
    insert_declaration(&mut doc, "plugins", name, table)?;
    manifest::parse(&doc.to_string(), paths)?;
    write_atomic(&paths.manifest(), doc.to_string().as_bytes())?;
    let at: Vec<String> = folders
        .iter()
        .map(|(agent, dir)| format!("{agent} from {}", if dir.is_empty() { "." } else { dir }))
        .collect();
    println!("Added plugin {name}: {}", at.join(", "));
    Ok(())
}

/// Inserts an entry the way `cargo add` does: in key order when the table is
/// already sorted, otherwise last.
fn insert_entry(table: &mut Table, name: &str, item: Item) {
    let sorted = table
        .iter()
        .filter(|(_, item)| item.is_value())
        .map(|(key, _)| key)
        .is_sorted();
    table.insert(name, item);
    if sorted {
        table.sort_values();
    }
}

fn configure_skills(
    paths: &Paths,
    names: &[String],
    options: &SkillOptions,
    resets: [bool; 3],
) -> Result<()> {
    options.validate(paths)?;
    let mut doc = read_manifest_doc(paths)?;
    let declarations = repositories::declarations(&doc);
    let coverage = Lock::load(&paths.lock())?.repositories;
    for name in names {
        let table = skills_table(&mut doc)?;
        if let Some(item) = table.get_mut(name) {
            options
                .apply(item, resets)
                .with_context(|| format!("configuring skill `{name}`"))?;
        } else {
            // A source's member gets its own entry, as a workspace dependency
            // gains `{ workspace = true, ... }`.
            let declaration = declarations
                .iter()
                .find(|d| d.selects(name, &coverage))
                .with_context(|| {
                    format!(
                        "`{name}` isn't in the manifest; wildcard members require a previous `skl sync`"
                    )
                })?;
            let mut entry = InlineTable::new();
            entry.insert("source", declaration.alias.as_str().into());
            let mut item = value(entry);
            options
                .apply(&mut item, resets)
                .with_context(|| format!("configuring skill `{name}`"))?;
            if item.as_inline_table().is_none_or(|t| t.len() == 1) {
                continue;
            }
            insert_entry(table, name, item);
        }
        repositories::settle(&mut doc, name, &coverage);
    }
    let text = doc.to_string();
    manifest::parse(&text, paths)?;
    write_atomic(&paths.manifest(), text.as_bytes())
}

fn remove(paths: &Paths, names: &[String]) -> Result<()> {
    let mut doc = read_manifest_doc(paths)?;
    let lock = Lock::load(&paths.lock())?;
    let (plugins, skills): (Vec<String>, Vec<String>) = names
        .iter()
        .cloned()
        .partition(|name| doc.get("plugins").and_then(|p| p.get(name)).is_some());
    if let Some(table) = doc.get_mut("plugins").and_then(Item::as_table_like_mut) {
        for name in &plugins {
            table.remove(name);
        }
        if table.is_empty() {
            doc.remove("plugins");
        }
    }
    if !skills.is_empty() {
        repositories::exclude_removed(paths, &mut doc, &lock.repositories, &skills)?;
    }
    manifest::parse(&doc.to_string(), paths)?;
    write_atomic(&paths.manifest(), doc.to_string().as_bytes())
}

fn list(paths: &Paths, active: &Active) -> Result<bool> {
    let mut manifest = manifest::load(paths)?;
    let lock = Lock::load(&paths.lock())?;
    repositories::append(paths, &mut manifest, &lock.repositories)?;
    export::append(paths, &mut manifest, &lock.commands, &Default::default())?;
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
    for alias in repositories::pending(paths, &manifest, &lock.repositories, active)? {
        println!("Source {alias}: wildcard coverage is missing or stale; run `skl sync`");
        healthy = false;
    }
    for (alias, app) in &manifest.apps {
        if app.skills.is_none()
            && app.coverage(alias, &lock.repositories).is_none()
            && !manifest::matches_os(&app.defaults.os)
        {
            println!(
                "Source {alias}: all skills selected (only on {})",
                app.defaults.os.join(", ")
            );
        }
    }
    for skill in &manifest.skills {
        let (origin, version) = match &skill.origin {
            Origin::Command(origin) => (
                format!("command: {}", origin.command[0]),
                lock.commands
                    .get(&skill.name)
                    .map_or_else(|| "not imported".into(), |e| e.snapshot.version.clone()),
            ),
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
            Origin::Local(dir) | Origin::OptionalLocal(dir) => {
                match export::member(&lock, &skill.name) {
                    Some(entry) if *dir == export::stored(paths, &skill.name, entry) => (
                        format!("app: {}", entry.repository.as_deref().unwrap_or_default()),
                        entry.snapshot.version.replace('\n', " / "),
                    ),
                    _ => (paths.tilde(dir), "local".to_string()),
                }
            }
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

    if !manifest.plugins.is_empty() {
        let installed = plugins::State::load(paths)?;
        let width = manifest
            .plugins
            .iter()
            .map(|p| p.name.len())
            .max()
            .unwrap_or(0);
        println!("\nPlugins:");
        for plugin in &manifest.plugins {
            let entry = lock.plugins.get(&plugin.name).filter(|e| e.matches(plugin));
            let version = entry.map_or_else(
                || "not locked".to_string(),
                |e| {
                    let commit = &e.commit[..7.min(e.commit.len())];
                    plugin
                        .git_ref
                        .as_ref()
                        .map_or(commit.to_string(), |r| format!("{r} {commit}"))
                },
            );
            let report = plugins::report(paths, active, plugin, &lock, &installed)?;
            let status = if !manifest::matches_os(&plugin.os) {
                format!("  (only on {})", plugin.os.join(", "))
            } else if !active.includes(&plugin.profiles) {
                format!("  (only for {})", plugin.profiles.join(", "))
            } else {
                let mut notes = Vec::new();
                let missing: Vec<&str> = report
                    .agents
                    .iter()
                    .filter(|a| a.status.needs_sync() && a.status != plugins::InstallStatus::Stale)
                    .map(|a| a.agent.as_str())
                    .collect();
                if !missing.is_empty() {
                    healthy = false;
                    notes.push(format!("not installed for {}", missing.join(", ")));
                }
                for agent in &report.agents {
                    match agent.status {
                        plugins::InstallStatus::Changed => {
                            healthy = false;
                            notes.push(format!("{} copy changed", agent.agent));
                        }
                        plugins::InstallStatus::Unavailable => notes.push(
                            plugins::Harness::named(&agent.agent)
                                .map(plugins::Harness::absence)
                                .unwrap_or_default(),
                        ),
                        _ => {}
                    }
                }
                if notes.is_empty() {
                    String::new()
                } else {
                    format!("  ({})", notes.join("; "))
                }
            };
            println!(
                "  {:<width$}  {version:<18} {}{status}",
                plugin.name, plugin.source
            );
        }
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
    if !unmanaged.is_empty() {
        println!("Snapshot an installed skill with `skl adopt NAME --from AGENT --dry-run`.");
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
