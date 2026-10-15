use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::ErrorKind;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use crate::git::{self, Git};
use crate::lock::{Lock, LockedSkill};
use crate::manifest::{self, GitOrigin, Origin, Skill};
use crate::paths::Paths;
use crate::profile::Active;

#[derive(Default)]
pub struct Options {
    pub dry_run: bool,
    pub force: bool,
    /// Fail instead of changing the lock, for checking that a machine matches it.
    pub locked: bool,
    /// Skills to move to the newest commit of their ref. Empty means all of them.
    pub update: Option<Vec<String>>,
    /// Print the full diff of each changed skill, not just which files changed.
    pub patch: bool,
    pub plan: Option<PathBuf>,
    pub apply_plan: Option<PathBuf>,
}

/// Brings the lock, the store and every agent's links in line with the manifest.
/// Returns false when some skill or link couldn't be put in place.
pub fn run(paths: &Paths, active: &Active, opts: &Options) -> Result<bool> {
    let temporary = (opts.dry_run || opts.plan.is_some())
        .then(tempfile::tempdir)
        .transpose()?;
    let git = Git::new(
        temporary
            .as_ref()
            .map_or_else(|| paths.repos(), |t| t.path().join("repos")),
    );
    let baseline = opts
        .plan
        .as_ref()
        .map(|_| crate::plan::Context::read(paths, active))
        .transpose()?;
    let saved = opts
        .apply_plan
        .as_ref()
        .map(|file| crate::plan::Plan::load(file, paths, active))
        .transpose()?;
    let options = Options {
        dry_run: opts.dry_run || opts.plan.is_some(),
        force: opts.force,
        locked: opts.locked,
        patch: opts.patch,
        update: saved
            .as_ref()
            .map(|p| p.names.clone())
            .or_else(|| opts.update.clone()),
        ..Default::default()
    };
    if opts.plan.is_some() || saved.is_some() {
        crate::plan::check_manifest(paths)?;
    }
    let (ok, lock) = run_inner(
        paths,
        active,
        &options,
        &git,
        saved.as_ref().map(|p| &p.target),
    )?;
    if let Some(file) = &opts.plan {
        if !ok {
            bail!("update plan was not saved because some skills or links failed");
        }
        crate::plan::Plan::save(
            file,
            paths,
            active,
            baseline.expect("planning captured a baseline"),
            options.update.unwrap_or_default(),
            lock,
            &git,
        )?;
        println!("Saved update plan: {}", paths.tilde(file));
    }
    Ok(ok)
}

fn run_inner(
    paths: &Paths,
    active: &Active,
    opts: &Options,
    git: &Git,
    planned: Option<&Lock>,
) -> Result<(bool, Lock)> {
    let mut manifest = manifest::load(paths)?;
    let old_lock = Lock::load(&paths.lock())?;
    let mut out = Report::new(opts.dry_run);

    let mut xcode_entries = BTreeMap::new();
    let mut xcode_all_complete = false;
    if let Some(config) = &manifest.xcode {
        xcode_entries = old_lock
            .xcode
            .iter()
            .filter(|(n, _)| config.selects(n))
            .map(|(n, e)| (n.clone(), e.clone()))
            .collect();
        xcode_all_complete = config.skills.is_none() && old_lock.xcode_all_complete;
        let unrelated_update = opts.update.as_ref().is_some_and(|names| {
            !names.is_empty()
                && names
                    .iter()
                    .all(|name| manifest.skills.iter().any(|s| &s.name == name))
        });
        if !config.enabled(active) {
            xcode_entries = old_lock.xcode.clone();
            xcode_all_complete = old_lock.xcode_all_complete;
        } else if config.skills.as_ref().is_some_and(|s| s.is_empty()) {
            if opts.locked && !old_lock.xcode.is_empty() {
                bail!("skills.lock still contains Xcode skills; run `skl sync` without --locked");
            }
        } else if !unrelated_update {
            if opts.locked {
                if xcode_entries != old_lock.xcode
                    || config
                        .skills
                        .as_ref()
                        .is_some_and(|names| names.iter().any(|n| !xcode_entries.contains_key(n)))
                    || config.skills.is_none() && !xcode_all_complete
                    || xcode_entries.iter().any(|(n, e)| {
                        !crate::xcode::skill_dir(paths, n, e)
                            .join("SKILL.md")
                            .is_file()
                    })
                {
                    bail!(
                        "Xcode exports are not cached for this selection; run `skl sync` without --locked"
                    );
                }
            } else if opts.dry_run {
                out.note("would run `xcrun agent skills export <staging-directory>` and select Xcode skills".into());
            } else {
                match crate::xcode::Export::run().and_then(|export| {
                    let (entries, errors) = export.install(
                        paths,
                        config,
                        &old_lock.xcode,
                        &manifest.skills,
                        opts.update.as_ref(),
                    )?;
                    let complete = config.skills.is_none() && export.covers_all(&entries);
                    Ok((entries, errors, complete))
                }) {
                    Ok((entries, errors, complete)) => {
                        xcode_all_complete = complete;
                        for (name, entry) in &entries {
                            if old_lock.xcode.get(name) != Some(entry) {
                                out.event(
                                    if old_lock.xcode.contains_key(name) {
                                        '↑'
                                    } else {
                                        '+'
                                    },
                                    name,
                                    format!("Xcode {}", entry.version.replace('\n', " / ")),
                                );
                            }
                        }
                        xcode_entries = entries;
                        for error in errors {
                            out.error("Xcode", &error);
                        }
                    }
                    Err(err) => out.error("Xcode", &format!("{err:#}")),
                }
            }
        }
    } else if opts.locked && !old_lock.xcode.is_empty() {
        bail!("skills.lock still contains Xcode skills; run `skl sync` without --locked");
    }
    crate::xcode::append_skills(paths, &mut manifest, &xcode_entries)?;

    // Excluded skills retain their lock entries across machines with different OSes
    // or profiles. Their owned links are removed by reconciliation below.
    let enabled = |skill: &Skill| skill.enabled(active);
    if let Some(names) = &opts.update {
        for name in names {
            match manifest.skills.iter().find(|s| &s.name == name) {
                None => bail!("`{name}` isn't in the manifest"),
                Some(skill) if !manifest::matches_os(&skill.os) => {
                    bail!("`{name}` is not enabled on {}", std::env::consts::OS)
                }
                Some(skill) if !enabled(skill) => {
                    bail!("`{name}` isn't in this machine's profiles")
                }
                Some(_) => {}
            }
        }
    }
    let wants_update = |name: &str| {
        opts.update
            .as_ref()
            .is_some_and(|names| names.is_empty() || names.iter().any(|n| n == name))
    };

    let git_skills: Vec<(&Skill, &GitOrigin)> = manifest
        .skills
        .iter()
        .filter(|s| enabled(s))
        .filter_map(|s| match &s.origin {
            Origin::Git(origin) => Some((s, origin)),
            Origin::Local(_) => None,
        })
        .collect();
    let stale: Vec<&(&Skill, &GitOrigin)> = git_skills
        .iter()
        .filter(|(s, origin)| {
            wants_update(&s.name)
                || !old_lock
                    .skills
                    .get(&s.name)
                    .is_some_and(|l| l.matches(origin))
        })
        .collect();

    if opts.locked {
        let mut drift: Vec<&str> = stale.iter().map(|(s, _)| s.name.as_str()).collect();
        drift.extend(
            old_lock
                .skills
                .keys()
                .filter(|n| !manifest.skills.iter().any(|s| &s.name == *n))
                .map(String::as_str),
        );
        if !drift.is_empty() {
            bail!(
                "skills.lock is out of date for {}: run `skl sync` without --locked",
                drift.join(", ")
            );
        }
    }

    // Resolve refs once per (url, ref) and fetch once per (url, commit).
    let refs: Vec<(String, Option<String>)> = if planned.is_some() {
        Vec::new()
    } else {
        dedup(
            stale
                .iter()
                .map(|(_, o)| (o.url.clone(), o.git_ref.clone())),
        )
    };
    let resolved: BTreeMap<_, _> = refs
        .iter()
        .cloned()
        .zip(par_map(&refs, |(url, r)| {
            git.resolve(url, r.as_deref()).map_err(|e| format!("{e:#}"))
        }))
        .collect();

    let mut commits: BTreeMap<String, Result<(String, String), String>> = BTreeMap::new();
    for (skill, origin) in &git_skills {
        let commit = if stale.iter().any(|(s, _)| s.name == skill.name) {
            if let Some(plan) = planned {
                Ok(plan.skills[&skill.name].commit.clone())
            } else {
                resolved[&(origin.url.clone(), origin.git_ref.clone())].clone()
            }
        } else {
            Ok(old_lock.skills[&skill.name].commit.clone())
        };
        commits.insert(skill.name.clone(), commit.map(|c| (origin.url.clone(), c)));
    }
    // Only each skill's own directory is unpacked, at store/<repo>/<commit>/<name>.
    let skill_dir = |url: &str, commit: &str, subdir: &str| {
        let dir = paths.store().join(git::cache_key(url)).join(commit);
        dir.join(store_name(subdir))
    };
    // A skill already in the store needs no network unless it must be located again.
    let fetches: Vec<(String, String)> = dedup(git_skills.iter().filter_map(|(skill, _)| {
        let (url, commit) = commits[&skill.name].as_ref().ok()?;
        let needed = stale.iter().any(|(s, _)| s.name == skill.name)
            || !skill_dir(url, commit, &old_lock.skills[&skill.name].subdir).exists();
        needed.then(|| (url.clone(), commit.clone()))
    }));
    let fetched: BTreeMap<_, _> = fetches
        .iter()
        .cloned()
        .zip(par_map(&fetches, |(url, commit)| {
            git.ensure_commit(url, commit).map_err(|e| format!("{e:#}"))
        }))
        .collect();

    // Build the new lock, keeping the old entry for any skill that failed so a flaky
    // network never drops it.
    let mut lock = Lock {
        version: old_lock.version,
        xcode_all_complete,
        xcode: xcode_entries,
        skills: manifest
            .skills
            .iter()
            .filter(|s| !enabled(s))
            .filter_map(|s| Some((s.name.clone(), old_lock.skills.get(&s.name)?.clone())))
            .collect(),
    };
    let mut failed = BTreeSet::new();
    let mut candidates = BTreeMap::new();
    for (skill, origin) in &git_skills {
        let name = &skill.name;
        let entry = if stale.iter().any(|(s, _)| &s.name == name) {
            locate(git, origin, &commits[name], &fetched, &mut candidates, name)
        } else {
            Ok(old_lock.skills[name].clone())
        };
        match entry {
            Ok(entry) => {
                match old_lock.skills.get(name) {
                    None => out.event(
                        '+',
                        name,
                        format!("{} @ {}", entry.source, short(&entry.commit)),
                    ),
                    Some(old) if old == &entry => {}
                    Some(old) => {
                        let (mark, head) = if old.commit != entry.commit {
                            (
                                '↑',
                                format!("{} → {}", short(&old.commit), short(&entry.commit)),
                            )
                        } else {
                            ('~', format!("{} @ {}", entry.source, short(&entry.commit)))
                        };
                        match describe_change(git, old, &entry, &origin.url, opts.patch) {
                            Ok(text) if text.trim().is_empty() => {
                                out.event(mark, name, format!("{head} (its files didn't change)"))
                            }
                            Ok(text) => {
                                out.event(mark, name, head);
                                out.block(&text);
                            }
                            Err(err) => {
                                out.event(mark, name, head);
                                out.note(format!("couldn't show what changed: {err:#}"));
                            }
                        }
                    }
                }
                lock.skills.insert(name.clone(), entry);
            }
            Err(err) => {
                out.error(name, &err);
                failed.insert(name.clone());
                if let Some(old) = old_lock.skills.get(name) {
                    lock.skills.insert(name.clone(), old.clone());
                }
            }
        }
    }
    for name in old_lock.skills.keys() {
        if !manifest.skills.iter().any(|s| &s.name == name) {
            out.event('-', name, String::new());
        }
    }

    if let Some(plan) = planned {
        for (name, entry) in &lock.skills {
            if !failed.contains(name) && plan.skills.get(name) != Some(entry) {
                bail!("update plan entry for `{name}` does not match the resolved skill");
            }
        }
    }
    let unpack: Vec<(String, String, String)> = dedup(
        git_skills
            .iter()
            .filter(|(s, _)| !failed.contains(&s.name))
            .filter_map(|(s, o)| {
                lock.skills
                    .get(&s.name)
                    .map(|l| (o.url.clone(), l.commit.clone(), l.subdir.clone()))
            })
            .filter(|(url, commit, subdir)| !skill_dir(url, commit, subdir).exists()),
    );
    if !opts.dry_run {
        let results = par_map(&unpack, |(url, commit, subdir)| {
            let repo = match fetched.get(&(url.clone(), commit.clone())) {
                Some(Ok(repo)) => repo.clone(),
                Some(Err(err)) => return Err(err.clone()),
                None => git
                    .ensure_commit(url, commit)
                    .map_err(|e| format!("{e:#}"))?,
            };
            unpack_skill(git, &repo, commit, subdir, &skill_dir(url, commit, subdir))
                .map_err(|e| format!("{e:#}"))
        });
        for ((url, commit, subdir), result) in unpack.iter().zip(results) {
            if let Err(err) = result {
                for (skill, origin) in &git_skills {
                    if origin.url == *url
                        && lock
                            .skills
                            .get(&skill.name)
                            .is_some_and(|l| &l.commit == commit && &l.subdir == subdir)
                    {
                        out.error(&skill.name, &err);
                        failed.insert(skill.name.clone());
                    }
                }
            }
        }
    }

    // A skill that failed keeps its previous lock entry, store content and links, so a
    // flaky network or a broken upstream never uninstalls something that worked.
    for name in &failed {
        match old_lock.skills.get(name) {
            Some(old) => lock.skills.insert(name.clone(), old.clone()),
            None => lock.skills.remove(name),
        };
    }
    let recorded = State::load(&paths.state())?;

    // Work out where every link should point.
    let mut desired = BTreeMap::new();
    for skill in manifest.skills.iter().filter(|s| enabled(s)) {
        if failed.contains(&skill.name) {
            for agent in &skill.agents {
                let link = agent.dir.join(&skill.name);
                if let Some(target) = recorded.get(&link) {
                    desired.insert(link, target.clone());
                }
            }
            continue;
        }
        let target = match &skill.origin {
            Origin::Git(origin) => {
                let entry = &lock.skills[&skill.name];
                skill_dir(&origin.url, &entry.commit, &entry.subdir)
            }
            Origin::Local(dir) => {
                if opts.dry_run
                    && manifest
                        .xcode
                        .as_ref()
                        .is_some_and(|c| c.selects(&skill.name))
                    && dir.starts_with(paths.store().join("xcode"))
                    && !dir.join("SKILL.md").is_file()
                {
                    continue;
                }
                if !dir.join("SKILL.md").is_file() {
                    out.error(
                        &skill.name,
                        &format!("{} has no SKILL.md", paths.tilde(dir)),
                    );
                    for agent in &skill.agents {
                        let link = agent.dir.join(&skill.name);
                        if let Some(target) = recorded.get(&link) {
                            desired.insert(link, target.clone());
                        }
                    }
                    continue;
                }
                dir.clone()
            }
        };
        for agent in &skill.agents {
            let link = agent.dir.join(&skill.name);
            if link != target {
                desired.insert(link, target.clone());
            }
        }
    }

    if opts.locked && lock != old_lock {
        bail!("skills.lock is out of date: run `skl sync` without --locked");
    }
    let state = reconcile(paths, &recorded, &desired, opts, &mut out)?;

    if !opts.dry_run {
        if lock != old_lock || !paths.lock().exists() {
            lock.save(&paths.lock())?;
        }
        state.save(&paths.state())?;
        let inactive: BTreeSet<&str> = manifest
            .skills
            .iter()
            .filter(|s| !enabled(s))
            .map(|s| s.name.as_str())
            .collect();
        collect_garbage(paths, &lock, &inactive, &state)?;
    }
    Ok((out.finish(manifest.skills.len()), lock))
}

/// Summarizes how a skill's own files changed between two lock entries, as a
/// diffstat or, with `patch`, the full diff.
pub(crate) fn describe_change(
    git: &Git,
    old: &LockedSkill,
    new: &LockedSkill,
    url: &str,
    patch: bool,
) -> Result<String> {
    if old.source != new.source {
        return Ok(format!("moved from {}", old.source));
    }
    let repo = git.ensure_commit(url, &old.commit)?;
    let tree = |commit: &str, subdir: &str| {
        if subdir.is_empty() {
            commit.to_string()
        } else {
            format!("{commit}:{subdir}")
        }
    };
    git.diff(
        &repo,
        &tree(&old.commit, &old.subdir),
        &tree(&new.commit, &new.subdir),
        patch,
    )
}

/// Finds the directory holding the skill at a freshly resolved commit.
fn locate(
    git: &Git,
    origin: &GitOrigin,
    commit: &Result<(String, String), String>,
    fetched: &BTreeMap<(String, String), Result<PathBuf, String>>,
    cache: &mut BTreeMap<(String, String), Vec<git::Candidate>>,
    name: &str,
) -> Result<LockedSkill, String> {
    let (url, commit) = commit.clone()?;
    let key = (url.clone(), commit.clone());
    let repo = fetched[&key].clone()?;
    if !cache.contains_key(&key) {
        let found = git
            .candidates(&repo, &commit)
            .map_err(|e| format!("{e:#}"))?;
        cache.insert(key.clone(), found);
    }
    let candidates = &cache[&key];

    let subdir = if let Some(dir) = &origin.subdir {
        if !candidates.iter().any(|c| &c.subdir == dir) {
            return Err(format!("{} has no SKILL.md in `{dir}`", origin.source));
        }
        dir.clone()
    } else {
        let by_name: Vec<_> = candidates.iter().filter(|c| c.name == name).collect();
        let by_dir: Vec<_> = candidates.iter().filter(|c| c.dir_name == name).collect();
        // Frontmatter names win over directory names, and a repo with a single skill
        // can be installed under any name.
        let matches = if !by_name.is_empty() {
            by_name
        } else if !by_dir.is_empty() {
            by_dir
        } else if candidates.len() == 1 {
            candidates.iter().collect()
        } else if candidates.is_empty() {
            return Err(format!("{} has no SKILL.md", origin.source));
        } else {
            let names: Vec<_> = candidates.iter().map(|c| c.name.as_str()).collect();
            return Err(format!(
                "{} has no skill named `{name}`; it has {}",
                origin.source,
                names.join(", ")
            ));
        };
        if let [one] = matches.as_slice() {
            one.subdir.clone()
        } else {
            let dirs: Vec<_> = matches.iter().map(|c| c.subdir.as_str()).collect();
            return Err(format!(
                "several skills in {} match `{name}` ({}): set `subdir`",
                origin.source,
                dirs.join(", ")
            ));
        }
    };
    Ok(LockedSkill {
        source: origin.source.clone(),
        git_ref: origin.git_ref.clone(),
        commit,
        subdir,
    })
}

/// Flattens a repo subdirectory into one store directory name, so a skill nested inside
/// another skill's directory never shares files with it. `@root` is the repo root.
pub(crate) fn store_name(subdir: &str) -> String {
    if subdir.is_empty() {
        return "@root".into();
    }
    subdir
        .replace('%', "%25")
        .replace('@', "%40")
        .replace('/', "%2F")
}

fn unpack_skill(git: &Git, repo: &Path, commit: &str, subdir: &str, dest: &Path) -> Result<()> {
    let parent = dest.parent().expect("store paths have a parent");
    fs::create_dir_all(parent)?;
    let name = dest
        .file_name()
        .expect("store paths have a name")
        .to_string_lossy();
    let tmp = parent.join(format!(".{name}.tmp-{}", std::process::id()));
    if tmp.exists() {
        fs::remove_dir_all(&tmp)?;
    }
    let result = git.export(repo, commit, subdir, &tmp).and_then(|_| {
        // Never publish a tree an agent can't read: SKILL.md must be a real file.
        let skill = tmp.join("SKILL.md");
        if !fs::symlink_metadata(&skill).is_ok_and(|m| m.is_file()) || fs::read(&skill).is_err() {
            bail!("SKILL.md at {} is not a readable file", short(commit));
        }
        Ok(fs::rename(&tmp, dest)?)
    });
    if result.is_err() {
        let _ = fs::remove_dir_all(&tmp);
    }
    result
}

/// The links skl created on this machine, with the target each was given.
pub fn owned_links(paths: &Paths) -> Result<BTreeMap<PathBuf, PathBuf>> {
    State::load(&paths.state())
}

#[derive(Default, Serialize, Deserialize)]
struct State {
    #[serde(default, rename = "link")]
    links: Vec<LinkRecord>,
}

#[derive(Serialize, Deserialize)]
struct LinkRecord {
    path: PathBuf,
    target: PathBuf,
}

impl State {
    fn load(path: &Path) -> Result<BTreeMap<PathBuf, PathBuf>> {
        let text = match fs::read_to_string(path) {
            Ok(text) => text,
            Err(err) if err.kind() == ErrorKind::NotFound => return Ok(BTreeMap::new()),
            Err(err) => return Err(err.into()),
        };
        let state: State =
            toml::from_str(&text).with_context(|| format!("invalid {}", path.display()))?;
        Ok(state
            .links
            .into_iter()
            .map(|l| (l.path, l.target))
            .collect())
    }

    fn save(&self, path: &Path) -> Result<()> {
        crate::write_atomic(path, toml::to_string(self)?.as_bytes())
    }
}

/// Creates, repoints and removes links. skl owns a link only when the state file
/// records it with the target it still has. Only owned links are repointed or removed;
/// anything else at a link's path is a conflict unless `--force` moves it aside.
fn reconcile(
    paths: &Paths,
    recorded: &BTreeMap<PathBuf, PathBuf>,
    desired: &BTreeMap<PathBuf, PathBuf>,
    opts: &Options,
    out: &mut Report,
) -> Result<State> {
    enum Action {
        Keep { owned: bool },
        Create,
        Repoint,
        Force,
        Conflict,
    }
    let owned = |path: &Path, current: &Path| recorded.get(path).is_some_and(|t| t == current);

    let mut plan = Vec::with_capacity(desired.len());
    for (path, target) in desired {
        let current = match fs::symlink_metadata(path) {
            Err(err) if err.kind() == ErrorKind::NotFound => None,
            Err(err) => return Err(err).with_context(|| format!("checking {}", path.display())),
            Ok(meta) => Some(meta),
        };
        let link_target = current
            .as_ref()
            .filter(|m| m.file_type().is_symlink())
            .and_then(|_| fs::read_link(path).ok());
        let action = match (&current, &link_target) {
            (None, _) => Action::Create,
            // Already right. A link someone else made stays theirs.
            (Some(_), Some(t)) if t == target => Action::Keep {
                owned: owned(path, t),
            },
            (Some(_), Some(t)) if owned(path, t) => Action::Repoint,
            (Some(_), _) if opts.force => Action::Force,
            (Some(_), _) => Action::Conflict,
        };
        plan.push((path, target, action));
    }

    // Record the links about to be made before making them, so a run that dies halfway
    // still knows which links it owns.
    let mut state = State::default();
    for (path, target, action) in &plan {
        if matches!(
            action,
            Action::Create | Action::Repoint | Action::Force | Action::Keep { owned: true }
        ) {
            state.links.push(LinkRecord {
                path: (*path).clone(),
                target: (*target).clone(),
            });
        }
    }
    if !opts.dry_run {
        let mut intent = State::default();
        intent.links.extend(
            recorded
                .iter()
                .filter(|(p, _)| !state.links.iter().any(|l| &l.path == *p))
                .map(|(p, t)| LinkRecord {
                    path: p.clone(),
                    target: t.clone(),
                }),
        );
        intent.links.extend(state.links.iter().map(|l| LinkRecord {
            path: l.path.clone(),
            target: l.target.clone(),
        }));
        intent.save(&paths.state())?;
    }

    for (path, target) in recorded.iter().filter(|(p, _)| !desired.contains_key(*p)) {
        if fs::read_link(path).is_ok_and(|current| current == *target) {
            if !opts.dry_run {
                fs::remove_file(path).with_context(|| format!("removing {}", path.display()))?;
            }
            out.unlinked += 1;
        }
    }

    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let mut moved = 0;
    for (path, target, action) in plan {
        match action {
            Action::Keep { .. } => {}
            Action::Create => {
                if !opts.dry_run {
                    fs::create_dir_all(path.parent().expect("links have a parent"))?;
                    symlink(target, path).with_context(|| format!("linking {}", path.display()))?;
                }
                out.linked += 1;
            }
            Action::Repoint => {
                if !opts.dry_run {
                    fs::remove_file(path)?;
                    symlink(target, path).with_context(|| format!("linking {}", path.display()))?;
                }
                out.relinked += 1;
            }
            Action::Force => {
                // Every move gets its own directory, created fresh, so no backup can
                // ever replace another.
                moved += 1;
                let slot = paths.backups().join(format!("{stamp}-{}-{moved}", std::process::id()));
                let backup = slot.join(path.file_name().expect("links have a name"));
                if !opts.dry_run {
                    fs::create_dir_all(paths.backups())?;
                    fs::create_dir(&slot).with_context(|| format!("creating backup {}", slot.display()))?;
                    fs::write(slot.join("ORIGINAL_PATH"), format!("{}\n", path.display()))?;
                    fs::rename(path, &backup)
                        .with_context(|| format!("moving {} to {}", path.display(), backup.display()))?;
                    symlink(target, path).with_context(|| format!("linking {}", path.display()))?;
                }
                out.note(format!("moved {} to {}", paths.tilde(path), paths.tilde(&backup)));
                out.linked += 1;
            }
            Action::Conflict => out.conflict(format!(
                "{} already exists and skl didn't create it: move it, or rerun with --force to back it up",
                paths.tilde(path)
            )),
        }
    }
    Ok(state)
}

/// Drops store commits and repo caches that neither the lock nor any owned link uses.
/// Sources come from the lock itself, so a skill kept on its old install after a
/// failed change of source keeps its files.
fn collect_garbage(
    paths: &Paths,
    lock: &Lock,
    inactive: &BTreeSet<&str>,
    state: &State,
) -> Result<()> {
    let store = paths.store();
    let mut live: BTreeSet<(String, String)> = lock
        .skills
        .iter()
        .filter(|(name, _)| !inactive.contains(name.as_str()))
        .filter_map(|(_, l)| {
            let url = manifest::source_url(&l.source).ok()?;
            Some((git::cache_key(&url), l.commit.clone()))
        })
        .collect();
    for (name, entry) in &lock.xcode {
        if !inactive.contains(name.as_str()) {
            live.insert(("xcode".into(), entry.digest.clone()));
        }
    }
    for link in &state.links {
        if let Ok(rest) = link.target.strip_prefix(&store) {
            let mut parts = rest
                .components()
                .map(|c| c.as_os_str().to_string_lossy().into_owned());
            if let (Some(key), Some(commit)) = (parts.next(), parts.next()) {
                live.insert((key, commit));
            }
        }
    }
    let keys: BTreeSet<&String> = live.iter().map(|(key, _)| key).collect();

    for repo in read_dirs(&store)? {
        let key = file_name(&repo);
        for commit in read_dirs(&repo)? {
            if !live.contains(&(key.clone(), file_name(&commit))) {
                fs::remove_dir_all(&commit)?;
            }
        }
        if fs::read_dir(&repo)?.next().is_none() {
            fs::remove_dir(&repo)?;
        }
    }
    for repo in read_dirs(&paths.repos())? {
        if !keys.contains(&file_name(&repo).trim_end_matches(".git").to_string()) {
            fs::remove_dir_all(&repo)?;
        }
    }
    Ok(())
}

fn read_dirs(dir: &Path) -> Result<Vec<PathBuf>> {
    match fs::read_dir(dir) {
        Ok(entries) => Ok(entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.is_dir() && !p.is_symlink())
            .collect()),
        Err(err) if err.kind() == ErrorKind::NotFound => Ok(Vec::new()),
        Err(err) => Err(err.into()),
    }
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

struct Report {
    dry_run: bool,
    added: usize,
    updated: usize,
    removed: usize,
    linked: usize,
    relinked: usize,
    unlinked: usize,
    problems: usize,
}

impl Report {
    fn new(dry_run: bool) -> Self {
        Self {
            dry_run,
            added: 0,
            updated: 0,
            removed: 0,
            linked: 0,
            relinked: 0,
            unlinked: 0,
            problems: 0,
        }
    }

    fn event(&mut self, mark: char, name: &str, detail: String) {
        match mark {
            '+' => self.added += 1,
            '-' => self.removed += 1,
            _ => self.updated += 1,
        }
        println!("{}", format!("{mark} {name:<32} {detail}").trim_end());
    }

    fn note(&self, message: String) {
        println!("  {message}");
    }

    fn block(&self, text: &str) {
        for line in text.trim_end().lines() {
            println!("{}", format!("    {line}").trim_end());
        }
    }

    fn error(&mut self, name: &str, message: &str) {
        self.problems += 1;
        eprintln!("! {name}: {message}");
    }

    fn conflict(&mut self, message: String) {
        self.problems += 1;
        eprintln!("! {message}");
    }

    fn finish(&self, total: usize) -> bool {
        let mut parts = Vec::new();
        for (count, label) in [
            (self.added, "added"),
            (self.updated, "updated"),
            (self.removed, "removed"),
            (self.linked, "links created"),
            (self.relinked, "links repointed"),
            (self.unlinked, "links removed"),
        ] {
            if count > 0 {
                parts.push(format!("{count} {label}"));
            }
        }
        let skills = if total == 1 { "skill" } else { "skills" };
        if parts.is_empty() && self.problems == 0 {
            println!("Up to date: {total} {skills}");
        } else {
            let lead = if self.dry_run {
                "Would apply"
            } else {
                "Synced"
            };
            let changes = if parts.is_empty() {
                "no changes".to_string()
            } else {
                parts.join(", ")
            };
            println!("{lead}: {changes} ({total} {skills})");
        }
        if self.problems > 0 {
            eprintln!(
                "{} problem{} need attention",
                self.problems,
                if self.problems == 1 { "" } else { "s" }
            );
        }
        self.problems == 0
    }
}

fn short(commit: &str) -> &str {
    &commit[..commit.len().min(7)]
}

fn dedup<T: Ord>(items: impl Iterator<Item = T>) -> Vec<T> {
    items.collect::<BTreeSet<_>>().into_iter().collect()
}

/// Runs `f` over `items` on a few threads, keeping results in order.
fn par_map<T: Sync, R: Send>(items: &[T], f: impl Fn(&T) -> R + Sync) -> Vec<R> {
    let next = AtomicUsize::new(0);
    let results: Mutex<Vec<Option<R>>> = Mutex::new((0..items.len()).map(|_| None).collect());
    std::thread::scope(|scope| {
        for _ in 0..items.len().min(8) {
            scope.spawn(|| {
                loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    let Some(item) = items.get(i) else { break };
                    let result = f(item);
                    results.lock().unwrap()[i] = Some(result);
                }
            });
        }
    });
    results
        .into_inner()
        .unwrap()
        .into_iter()
        .map(|r| r.expect("every item ran"))
        .collect()
}
