use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use tempfile::TempDir;

/// A throwaway HOME plus local git repos to install skills from.
struct Env {
    _tmp: TempDir,
    home: PathBuf,
    remotes: PathBuf,
}

impl Env {
    fn new() -> Self {
        let tmp = TempDir::new().unwrap();
        let home = tmp.path().join("home");
        let remotes = tmp.path().join("remotes");
        fs::create_dir_all(&home).unwrap();
        fs::create_dir_all(&remotes).unwrap();
        Self {
            _tmp: tmp,
            home,
            remotes,
        }
    }

    fn skl(&self, args: &[&str]) -> Output {
        self.skl_with_env(args, &[])
    }

    fn skl_with_env(&self, args: &[&str], vars: &[(&str, &std::ffi::OsStr)]) -> Output {
        let out = Command::new(env!("CARGO_BIN_EXE_skl"))
            .args(args)
            .current_dir(&self.home)
            .env("HOME", &self.home)
            .env_remove("XDG_CONFIG_HOME")
            .env_remove("XDG_DATA_HOME")
            .env_remove("SKL_CONFIG_DIR")
            .env_remove("SKL_DATA_DIR")
            .env_remove("SKL_PROFILE")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .envs(vars.iter().copied())
            .output()
            .unwrap();
        eprintln!(
            "$ skl {}\n{}{}",
            args.join(" "),
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        out
    }

    fn ok(&self, args: &[&str]) -> String {
        let out = self.skl(args);
        assert!(out.status.success(), "skl {args:?} failed");
        String::from_utf8_lossy(&out.stdout).into_owned()
    }

    fn fails(&self, args: &[&str]) -> String {
        let out = self.skl(args);
        assert!(!out.status.success(), "skl {args:?} should fail");
        String::from_utf8_lossy(&out.stderr).into_owned()
    }

    /// Creates a repo with a SKILL.md in each `(dir, frontmatter name)`, returning its URL.
    fn repo(&self, name: &str, skills: &[(&str, &str)]) -> String {
        let dir = self.remotes.join(name);
        fs::create_dir_all(&dir).unwrap();
        git(&dir, &["init", "-q", "-b", "main"]);
        for (subdir, skill) in skills {
            write_skill(&dir.join(subdir), skill, "v1");
        }
        commit(&dir, "initial");
        format!("file://{}", dir.display())
    }

    fn remote(&self, name: &str) -> PathBuf {
        self.remotes.join(name)
    }

    fn config(&self, file: &str) -> PathBuf {
        self.home.join(".config/skl").join(file)
    }

    fn write_manifest(&self, text: &str) {
        let path = self.config("skills.toml");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    fn claude(&self, name: &str) -> PathBuf {
        self.home.join(".claude/skills").join(name)
    }

    fn codex(&self, name: &str) -> PathBuf {
        self.home.join(".agents/skills").join(name)
    }

    fn store_commits(&self) -> usize {
        let store = self.home.join(".local/share/skl/store");
        let Ok(repos) = fs::read_dir(store) else {
            return 0;
        };
        repos
            .flatten()
            .map(|r| fs::read_dir(r.path()).unwrap().count())
            .sum()
    }
}

fn write_skill(dir: &Path, name: &str, body: &str) {
    fs::create_dir_all(dir).unwrap();
    fs::write(
        dir.join("SKILL.md"),
        format!("---\nname: {name}\ndescription: test\n---\n{body}\n"),
    )
    .unwrap();
}

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args([
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@t",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn commit(dir: &Path, message: &str) -> String {
    git(dir, &["add", "-A"]);
    git(dir, &["commit", "-q", "-m", message]);
    git(dir, &["rev-parse", "HEAD"])
}

fn body(link: &Path) -> String {
    fs::read_to_string(link.join("SKILL.md")).unwrap()
}

#[test]
fn add_links_into_every_agent_and_writes_lock() {
    let env = Env::new();
    let url = env.repo("notes", &[("", "notes")]);
    let head = git(&env.remote("notes"), &["rev-parse", "HEAD"]);

    env.ok(&["add", &url]);

    let manifest = fs::read_to_string(env.config("skills.toml")).unwrap();
    assert!(
        manifest.contains(&format!("notes = \"{url}\"")),
        "{manifest}"
    );
    let lock = fs::read_to_string(env.config("skills.lock")).unwrap();
    assert!(lock.contains(&head), "{lock}");
    assert!(env.claude("notes").is_symlink());
    assert!(env.codex("notes").is_symlink());
    assert!(body(&env.claude("notes")).contains("v1"));

    let again = env.ok(&["sync"]);
    assert!(again.starts_with("Up to date: 1 skill"), "{again}");
    assert_eq!(lock, fs::read_to_string(env.config("skills.lock")).unwrap());
}

#[test]
fn lock_pins_commits_until_update() {
    let env = Env::new();
    let url = env.repo("notes", &[("", "notes")]);
    env.ok(&["add", &url]);

    write_skill(&env.remote("notes"), "notes", "v2");
    let new_head = commit(&env.remote("notes"), "v2");

    env.ok(&["sync"]);
    assert!(
        body(&env.claude("notes")).contains("v1"),
        "sync must not move a locked skill"
    );

    let out = env.ok(&["update"]);
    assert!(out.contains('↑'), "{out}");
    assert!(body(&env.claude("notes")).contains("v2"));
    assert!(
        fs::read_to_string(env.config("skills.lock"))
            .unwrap()
            .contains(&new_head)
    );
    assert_eq!(
        env.store_commits(),
        1,
        "the old commit should be garbage collected"
    );
}

#[test]
fn second_machine_restores_from_manifest_and_lock() {
    let first = Env::new();
    let url = first.repo("notes", &[("", "notes")]);
    first.ok(&["add", &url]);
    let pinned = git(&first.remote("notes"), &["rev-parse", "HEAD"]);
    write_skill(&first.remote("notes"), "notes", "v2");
    commit(&first.remote("notes"), "v2");

    let second = Env::new();
    for file in ["skills.toml", "skills.lock"] {
        fs::create_dir_all(second.config(file).parent().unwrap()).unwrap();
        fs::copy(first.config(file), second.config(file)).unwrap();
    }
    second.ok(&["sync", "--locked"]);
    assert!(
        body(&second.claude("notes")).contains("v1"),
        "must install {pinned}, not the newer HEAD"
    );
}

#[test]
fn remove_prunes_links_and_store() {
    let env = Env::new();
    let url = env.repo("pack", &[("skills/a", "alpha"), ("skills/b", "beta")]);
    env.ok(&["add", &url, "--all"]);
    assert!(env.claude("alpha").is_symlink() && env.claude("beta").is_symlink());

    env.ok(&["remove", "alpha"]);
    assert!(!env.claude("alpha").exists() && !env.codex("alpha").exists());
    assert!(env.claude("beta").is_symlink());

    // Deleting a manifest entry by hand prunes it too.
    env.write_manifest("[skills]\n");
    env.ok(&["sync"]);
    assert!(!env.claude("beta").exists());
    assert_eq!(env.store_commits(), 0);
    assert!(
        !fs::read_to_string(env.config("skills.lock"))
            .unwrap()
            .contains("beta")
    );
}

#[test]
fn never_clobbers_what_it_did_not_create() {
    let env = Env::new();
    let url = env.repo("docs", &[("", "docs")]);
    let mine = env.claude("docs");
    fs::create_dir_all(&mine).unwrap();
    fs::write(mine.join("NOTES.md"), "my edits").unwrap();

    let err = env.fails(&["add", &url]);
    assert!(err.contains("skl didn't create it"), "{err}");
    assert!(
        mine.join("NOTES.md").is_file(),
        "existing directory must be untouched"
    );
    assert!(
        env.codex("docs").is_symlink(),
        "other agents still get the skill"
    );

    env.ok(&["sync", "--force"]);
    assert!(env.claude("docs").is_symlink());
    let backups = env.home.join(".local/share/skl/backup");
    let found = walk(&backups)
        .into_iter()
        .any(|p| p.ends_with("docs/NOTES.md"));
    assert!(
        found,
        "the directory should be moved into the backup folder"
    );
}

#[test]
fn picks_skills_by_frontmatter_name() {
    let env = Env::new();
    let url = env.repo(
        "pack",
        &[
            ("skills/migrate", "migrating-projects"),
            ("skills/fix", "fix-flaky-tests"),
        ],
    );

    let err = env.fails(&["add", &url]);
    assert!(err.contains("pick with --skill"), "{err}");

    env.ok(&["add", &url, "--skill", "migrating-projects"]);
    assert!(
        fs::read_to_string(env.config("skills.lock"))
            .unwrap()
            .contains("subdir = \"skills/migrate\"")
    );
    assert!(env.claude("migrating-projects").join("SKILL.md").is_file());

    let err = env.fails(&["add", &url, "--skill", "nope"]);
    assert!(err.contains("no skill named `nope`"), "{err}");
}

#[test]
fn locked_fails_when_manifest_moves_ahead_of_lock() {
    let env = Env::new();
    let url = env.repo("notes", &[("", "notes")]);
    env.ok(&["add", &url]);
    let other = env.repo("other", &[("", "other")]);
    env.write_manifest(&format!(
        "[skills]\nnotes = \"{url}\"\nother = \"{other}\"\n"
    ));

    let err = env.fails(&["sync", "--locked"]);
    assert!(err.contains("out of date for other"), "{err}");
    assert!(!env.claude("other").exists());
}

#[test]
fn follows_tags_and_branches() {
    let env = Env::new();
    let url = env.repo("notes", &[("", "notes")]);
    git(&env.remote("notes"), &["tag", "-a", "v1.0", "-m", "v1.0"]);
    write_skill(&env.remote("notes"), "notes", "v2");
    commit(&env.remote("notes"), "v2");

    env.ok(&["add", &url, "--ref", "v1.0"]);
    assert!(body(&env.claude("notes")).contains("v1"));
    env.ok(&["update"]);
    assert!(
        body(&env.claude("notes")).contains("v1"),
        "a tag doesn't move on update"
    );

    let err = env.fails(&["add", &env.repo("x", &[("", "x")]), "--ref", "nope"]);
    assert!(err.contains("no branch or tag named `nope`"), "{err}");
}

#[test]
fn local_skills_link_straight_to_their_directory() {
    let env = Env::new();
    let dir = env.home.join("dotfiles/skills/mine");
    write_skill(&dir, "mine", "local");

    env.ok(&["add", "dotfiles/skills/mine"]);
    let manifest = fs::read_to_string(env.config("skills.toml")).unwrap();
    assert!(
        manifest.contains("path = \"~/dotfiles/skills/mine\""),
        "{manifest}"
    );
    assert_eq!(fs::read_link(env.claude("mine")).unwrap(), dir);

    // Edits show up without a sync.
    write_skill(&dir, "mine", "edited");
    assert!(body(&env.claude("mine")).contains("edited"));
}

#[test]
fn agents_can_be_limited_per_skill() {
    let env = Env::new();
    let url = env.repo("notes", &[("", "notes")]);
    env.write_manifest(&format!(
        "agents = [\"claude-code\"]\n[skills]\nnotes = \"{url}\"\nboth = {{ source = \"{url}\", agents = [\"claude-code\", \"codex\"] }}\n"
    ));
    env.ok(&["sync"]);
    assert!(env.claude("notes").is_symlink() && !env.codex("notes").exists());
    assert!(env.codex("both").is_symlink());

    env.write_manifest(&format!(
        "agents = [\"claude-code\"]\n[skills]\nnotes = \"{url}\"\nboth = \"{url}\"\n"
    ));
    env.ok(&["sync"]);
    assert!(
        !env.codex("both").exists(),
        "dropping an agent unlinks it there"
    );
}

#[test]
fn rejects_bad_manifests() {
    let env = Env::new();
    env.write_manifest("[skills]\nx = { source = \"a/b\", path = \"c\" }\n");
    assert!(
        env.fails(&["sync"])
            .contains("exactly one of `source` or `path`")
    );
    env.write_manifest("agents = [\"emacs\"]\n");
    assert!(env.fails(&["sync"]).contains("unknown agent `emacs`"));
    env.write_manifest("[skills]\n\"../x\" = \"a/b\"\n");
    assert!(env.fails(&["sync"]).contains("not a valid skill name"));
    env.write_manifest("skils = {}\n");
    assert!(env.fails(&["sync"]).contains("unknown key `skils`"));
}

#[test]
fn dry_run_changes_nothing() {
    let env = Env::new();
    let url = env.repo("notes", &[("", "notes")]);
    env.write_manifest(&format!("[skills]\nnotes = \"{url}\"\n"));
    let out = env.ok(&["sync", "--dry-run"]);
    assert!(out.contains("Would apply"), "{out}");
    assert!(!env.config("skills.lock").exists());
    assert!(!env.claude("notes").exists());
}

fn walk(dir: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                found.extend(walk(&path));
            } else {
                found.push(path);
            }
        }
    }
    found
}

#[test]
fn root_and_nested_skills_from_one_repo_coexist() {
    let env = Env::new();
    let url = env.repo("mixed", &[("", "top"), ("skills/inner", "inner")]);
    env.ok(&["add", &url, "--skill", "inner"]);
    env.ok(&["add", &url, "--skill", "top"]);
    assert!(body(&env.claude("top")).contains("name: top"));
    assert!(body(&env.claude("inner")).contains("name: inner"));
}

#[test]
fn prefers_skills_outside_hidden_mirrors() {
    let env = Env::new();
    let url = env.repo(
        "focus",
        &[(".tool/skills/focus", "focus"), ("skills/focus", "focus")],
    );
    env.ok(&["add", &url]);
    assert!(
        fs::read_to_string(env.config("skills.lock"))
            .unwrap()
            .contains("subdir = \"skills/focus\"")
    );
}

#[test]
fn failed_update_keeps_the_working_install() {
    let env = Env::new();
    let url = env.repo("notes", &[("", "notes")]);
    env.ok(&["add", &url]);
    let lock = fs::read_to_string(env.config("skills.lock")).unwrap();

    // The remote disappears, as if the network were down.
    fs::rename(env.remote("notes"), env.remotes.join("gone")).unwrap();
    env.fails(&["update"]);
    let offline = env.ok(&["sync"]);
    assert!(
        offline.starts_with("Up to date"),
        "sync needs no network once installed"
    );

    assert!(
        body(&env.claude("notes")).contains("v1"),
        "links must survive a failed update"
    );
    assert!(body(&env.codex("notes")).contains("v1"));
    assert_eq!(lock, fs::read_to_string(env.config("skills.lock")).unwrap());
}

#[test]
fn missing_local_skill_keeps_its_links() {
    let env = Env::new();
    let dir = env.home.join("mine");
    write_skill(&dir, "mine", "local");
    env.ok(&["add", "mine"]);
    fs::remove_file(dir.join("SKILL.md")).unwrap();
    env.fails(&["sync"]);
    assert!(env.claude("mine").is_symlink());
}

#[test]
fn nested_skill_does_not_hide_its_parent() {
    let env = Env::new();
    let url = env.repo("pack", &[("pack", "parent"), ("pack/child", "child")]);
    env.ok(&["add", &url, "--skill", "child"]);
    env.ok(&["add", &url, "--skill", "parent"]);
    assert!(body(&env.claude("parent")).contains("name: parent"));
    assert!(body(&env.claude("child")).contains("name: child"));
    assert!(
        env.claude("parent").join("child/SKILL.md").is_file(),
        "parent keeps its full tree"
    );
}

#[test]
fn leaves_user_links_into_the_store_alone() {
    let env = Env::new();
    let url = env.repo("notes", &[("", "notes")]);
    env.ok(&["add", &url]);
    let alias = env.claude("my-alias");
    std::os::unix::fs::symlink(fs::read_link(env.claude("notes")).unwrap(), &alias).unwrap();

    env.ok(&["sync"]);
    assert!(
        alias.is_symlink(),
        "a link the user made must not be pruned"
    );
}

#[test]
fn symlinked_manifest_and_lock_stay_linked() {
    let env = Env::new();
    let url = env.repo("notes", &[("", "notes")]);
    let dotfiles = env.home.join("dotfiles/skl");
    fs::create_dir_all(&dotfiles).unwrap();
    fs::write(dotfiles.join("skills.toml"), "[skills]\n").unwrap();
    fs::write(dotfiles.join("skills.lock"), "version = 1\n").unwrap();
    fs::create_dir_all(env.home.join(".config/skl")).unwrap();
    for file in ["skills.toml", "skills.lock"] {
        std::os::unix::fs::symlink(dotfiles.join(file), env.config(file)).unwrap();
    }

    env.ok(&["add", &url]);
    for file in ["skills.toml", "skills.lock"] {
        assert!(env.config(file).is_symlink(), "{file} must stay a symlink");
        assert!(
            fs::read_to_string(dotfiles.join(file))
                .unwrap()
                .contains("notes")
        );
    }
}

#[test]
fn backups_never_overwrite_each_other() {
    let env = Env::new();
    let url = env.repo("notes", &[("", "notes")]);
    // Two agent directories whose names used to map to the same backup path.
    env.write_manifest(&format!(
        "agents = [\"~/a/b\", \"~/a_b\"]\n[skills]\nnotes = \"{url}\"\n"
    ));
    for (dir, text) in [("a/b", "first"), ("a_b", "second")] {
        let path = env.home.join(dir);
        fs::create_dir_all(&path).unwrap();
        fs::write(path.join("notes"), text).unwrap();
    }
    env.ok(&["sync", "--force"]);

    let mut saved: Vec<String> = walk(&env.home.join(".local/share/skl/backup"))
        .into_iter()
        .filter(|p| p.ends_with("notes"))
        .map(|p| fs::read_to_string(p).unwrap())
        .collect();
    saved.sort();
    assert_eq!(saved, ["first", "second"]);
}

#[test]
fn concurrent_runs_take_turns() {
    let env = Env::new();
    let urls: Vec<String> = (0..4)
        .map(|i| env.repo(&format!("s{i}"), &[("", &format!("s{i}"))]))
        .collect();
    let mut manifest = String::from("[skills]\n");
    for (i, url) in urls.iter().enumerate() {
        manifest.push_str(&format!("s{i} = \"{url}\"\n"));
    }
    env.write_manifest(&manifest);

    let runs: Vec<_> = (0..4)
        .map(|_| {
            Command::new(env!("CARGO_BIN_EXE_skl"))
                .arg("sync")
                .current_dir(&env.home)
                .env("HOME", &env.home)
                .env_remove("SKL_CONFIG_DIR")
                .env_remove("SKL_DATA_DIR")
                .env_remove("SKL_PROFILE")
                .env_remove("XDG_CONFIG_HOME")
                .env_remove("XDG_DATA_HOME")
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
                .unwrap()
        })
        .collect();
    for mut run in runs {
        assert!(run.wait().unwrap().success());
    }
    for i in 0..4 {
        assert!(env.claude(&format!("s{i}")).join("SKILL.md").is_file());
    }
}

#[test]
fn failed_source_change_keeps_old_files() {
    let env = Env::new();
    let url = env.repo("notes", &[("", "notes")]);
    env.ok(&["add", &url]);
    let missing = format!("file://{}", env.remotes.join("nowhere").display());

    env.write_manifest(&format!("[skills]\nnotes = \"{missing}\"\n"));
    env.fails(&["sync"]);
    assert!(
        body(&env.claude("notes")).contains("v1"),
        "old install must keep its files"
    );

    env.write_manifest("[skills]\nnotes = { path = \"~/not-there\" }\n");
    env.fails(&["sync"]);
    assert!(body(&env.claude("notes")).contains("v1"));
}

#[test]
fn matching_link_made_by_user_stays_theirs() {
    let env = Env::new();
    let url = env.repo("notes", &[("", "notes")]);
    env.write_manifest(&format!(
        "agents = [\"claude-code\"]\n[skills]\nnotes = \"{url}\"\n"
    ));
    env.ok(&["sync"]);
    let target = fs::read_link(env.claude("notes")).unwrap();
    fs::create_dir_all(env.codex("notes").parent().unwrap()).unwrap();
    std::os::unix::fs::symlink(&target, env.codex("notes")).unwrap();

    env.write_manifest(&format!("[skills]\nnotes = \"{url}\"\n"));
    env.ok(&["sync"]);
    env.write_manifest("[skills]\n");
    env.ok(&["sync"]);
    assert!(!env.claude("notes").exists(), "skl's own link is pruned");
    assert!(env.codex("notes").is_symlink(), "the user's link is not");
}

#[test]
fn unrecorded_link_into_store_is_a_conflict() {
    let env = Env::new();
    let url = env.repo("notes", &[("", "notes")]);
    let other = env.repo("other", &[("", "other")]);
    env.ok(&["add", &url]);
    std::os::unix::fs::symlink(
        fs::read_link(env.claude("notes")).unwrap(),
        env.claude("other"),
    )
    .unwrap();

    let err = env.fails(&["add", &other]);
    assert!(err.contains("skl didn't create it"), "{err}");
    assert!(
        body(&env.claude("other")).contains("name: notes"),
        "user link untouched"
    );
}

#[test]
fn update_to_broken_skill_keeps_working_one() {
    let env = Env::new();
    let url = env.repo("notes", &[("", "notes")]);
    env.ok(&["add", &url]);
    let remote = env.remote("notes");
    fs::remove_file(remote.join("SKILL.md")).unwrap();
    std::os::unix::fs::symlink("/nonexistent/SKILL.md", remote.join("SKILL.md")).unwrap();
    commit(&remote, "broken");

    env.fails(&["update"]);
    assert!(body(&env.claude("notes")).contains("v1"));
}

fn skl_profile_env(env: &Env, profile: &str, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_skl"))
        .args(args)
        .current_dir(&env.home)
        .env("HOME", &env.home)
        .env_remove("XDG_CONFIG_HOME")
        .env_remove("XDG_DATA_HOME")
        .env("SKL_PROFILE", profile)
        .output()
        .unwrap()
}

#[test]
fn profiles_limit_which_skills_install() {
    let env = Env::new();
    let everywhere = env.repo("everywhere", &[("", "everywhere")]);
    let personal = env.repo("personal", &[("", "personal")]);
    env.write_manifest(&format!(
        "[skills]\neverywhere = \"{everywhere}\"\npersonal = {{ source = \"{personal}\", profiles = [\"personal\"] }}\n"
    ));

    // No profile: only the skill without `profiles`.
    env.ok(&["sync"]);
    assert!(env.claude("everywhere").is_symlink());
    assert!(!env.claude("personal").exists());
    let lock = fs::read_to_string(env.config("skills.lock")).unwrap();
    assert!(
        !lock.contains("[skills.personal]"),
        "inactive skills aren't resolved"
    );

    // Flag, then the machine setting.
    env.ok(&["sync", "--profile", "personal"]);
    assert!(env.claude("personal").is_symlink());
    env.ok(&["profile", "set", "personal"]);
    assert!(
        env.ok(&["profile"])
            .contains("personal (this machine's setting)")
    );
    env.ok(&["sync"]);
    assert!(env.claude("personal").is_symlink());
    let with_personal = fs::read_to_string(env.config("skills.lock")).unwrap();

    // A machine without the profile prunes the links but leaves the lock entry alone.
    env.ok(&["profile", "clear"]);
    write_skill(&env.remote("personal"), "personal", "v2");
    commit(&env.remote("personal"), "v2");
    env.ok(&["update"]);
    assert!(!env.claude("personal").exists(), "owned links are pruned");
    assert_eq!(
        with_personal,
        fs::read_to_string(env.config("skills.lock")).unwrap()
    );
    let err = env.fails(&["update", "personal"]);
    assert!(err.contains("isn't in this machine's profiles"), "{err}");

    // SKL_PROFILE wins over the machine setting.
    let out = skl_profile_env(&env, "personal", &["sync"]);
    assert!(out.status.success());
    assert!(
        body(&env.claude("personal")).contains("v1"),
        "still on the locked commit"
    );
}

#[test]
fn rejects_bad_profiles() {
    let env = Env::new();
    env.write_manifest("[skills]\nx = { source = \"a/b\", profiles = [] }\n");
    assert!(env.fails(&["sync"]).contains("non-empty array"));
    assert!(
        env.fails(&["profile", "set", "../x"])
            .contains("invalid profile")
    );
}

#[test]
fn update_shows_what_changed() {
    let env = Env::new();
    let url = env.repo("pack", &[("skills/a", "alpha"), ("skills/b", "beta")]);
    env.ok(&["add", &url, "--all"]);

    write_skill(
        &env.remote("pack").join("skills/a"),
        "alpha",
        "new instructions",
    );
    fs::write(env.remote("pack").join("skills/a/extra.md"), "more").unwrap();
    commit(&env.remote("pack"), "change alpha");

    let preview = env.ok(&["update", "--dry-run", "--patch"]);
    assert!(preview.contains("SKILL.md"), "{preview}");
    assert!(
        preview.contains("+new instructions"),
        "dry run shows the patch: {preview}"
    );
    assert!(
        preview.contains("its files didn't change"),
        "beta moved commits only: {preview}"
    );
    assert!(
        body(&env.claude("alpha")).contains("v1"),
        "dry run applies nothing"
    );

    let applied = env.ok(&["update"]);
    assert!(applied.contains("extra.md"), "{applied}");
    assert!(
        !applied.contains("+new instructions"),
        "no patch without --patch"
    );
    assert!(body(&env.claude("alpha")).contains("new instructions"));
}

#[test]
fn list_shows_unmanaged_skills() {
    let env = Env::new();
    let url = env.repo("notes", &[("", "notes")]);
    env.ok(&["add", &url]);
    fs::create_dir_all(env.claude("from-an-app")).unwrap();
    std::os::unix::fs::symlink("/Applications/Some.app/skill", env.codex("app-link")).unwrap();
    fs::write(env.claude(".DS_Store"), "").unwrap();

    let out = env.ok(&["list"]);
    let (managed, unmanaged) = out.split_once("Not managed by skl:").expect(&out);
    assert!(managed.contains("notes"));
    assert!(
        unmanaged.contains("from-an-app") && unmanaged.contains("directory"),
        "{out}"
    );
    assert!(unmanaged.contains("app-link") && unmanaged.contains("/Applications/Some.app/skill"));
    assert!(
        !unmanaged.contains("notes"),
        "skl's own links aren't listed"
    );
    assert!(!unmanaged.contains(".DS_Store"));
}

#[test]
fn xdg_directories_and_skl_overrides() {
    for use_override in [false, true] {
        let env = Env::new();
        let source = env.repo("skills", &[("notes", "notes")]);
        let xdg_config = env.home.join("xdg config");
        let xdg_data = env.home.join("xdg data");
        let own_config = env.home.join("own config");
        let own_data = env.home.join("own data");
        let config = if use_override {
            own_config.clone()
        } else {
            xdg_config.join("skl")
        };
        let data = if use_override {
            own_data.clone()
        } else {
            xdg_data.join("skl")
        };
        fs::create_dir_all(&config).unwrap();
        fs::write(
            config.join("skills.toml"),
            format!("[skills.notes]\nsource = {source:?}\nsubdir = \"notes\"\n"),
        )
        .unwrap();
        let mut vars = vec![
            ("XDG_CONFIG_HOME", xdg_config.as_os_str()),
            ("XDG_DATA_HOME", xdg_data.as_os_str()),
            ("SKL_CONFIG_DIR", std::ffi::OsStr::new("")),
            ("SKL_DATA_DIR", std::ffi::OsStr::new("")),
        ];
        if use_override {
            vars[2].1 = own_config.as_os_str();
            vars[3].1 = own_data.as_os_str();
        }
        let dry = env.skl_with_env(&["sync", "--dry-run"], &vars);
        assert!(dry.status.success(), "{dry:?}");
        assert!(!data.join("state.toml").exists());
        assert!(!data.join("store").exists());
        assert!(!env.codex("notes").exists());
        assert!(!config.join("skills.lock").exists());
        let out = env.skl_with_env(&["sync"], &vars);
        assert!(out.status.success(), "{out:?}");
        assert!(config.join("skills.lock").is_file());
        assert!(data.join("state.toml").is_file());
        assert!(data.join("repos").is_dir());
        let target = fs::read_link(env.codex("notes")).unwrap();
        assert!(target.starts_with(data.join("store")));
        assert!(target.join("SKILL.md").is_file());
        assert!(!env.home.join(".config/skl").exists());
        assert!(!env.home.join(".local/share/skl").exists());
        if use_override {
            assert!(!xdg_config.exists());
            assert!(!xdg_data.exists());
        }
        let locked = env.skl_with_env(&["sync", "--locked"], &vars);
        assert!(locked.status.success(), "{locked:?}");
    }
}

#[test]
fn empty_and_relative_xdg_directories_use_defaults() {
    for value in ["", "relative", "~/not-expanded"] {
        let env = Env::new();
        write_skill(&env.home.join("local"), "notes", "local");
        env.write_manifest("[skills.notes]\npath = \"~/local\"\n");
        let out = env.skl_with_env(
            &["sync"],
            &[
                ("XDG_CONFIG_HOME", std::ffi::OsStr::new(value)),
                ("XDG_DATA_HOME", std::ffi::OsStr::new(value)),
                ("SKL_CONFIG_DIR", std::ffi::OsStr::new("")),
                ("SKL_DATA_DIR", std::ffi::OsStr::new("")),
            ],
        );
        assert!(out.status.success(), "{out:?}");
        assert!(env.home.join(".local/share/skl/state.toml").is_file());
        assert!(env.codex("notes").join("SKILL.md").is_file());
        assert!(!env.home.join("relative").exists());
        assert!(!env.home.join("~").exists());
    }
}

fn fake_xcrun() -> &'static Path {
    static BIN: std::sync::OnceLock<TempDir> = std::sync::OnceLock::new();
    BIN.get_or_init(|| {
        let dir = tempfile::tempdir().unwrap();
        let output = Command::new("rustc")
            .arg(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/fixtures/xcrun.rs"
            ))
            .arg("-o")
            .arg(dir.path().join("xcrun"))
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        dir
    })
    .path()
}

impl Env {
    fn xcode(&self, args: &[&str]) -> Output {
        let path = std::env::join_paths(
            std::iter::once(fake_xcrun().to_path_buf())
                .chain(std::env::split_paths(&std::env::var_os("PATH").unwrap())),
        )
        .unwrap();
        self.skl_with_env(
            args,
            &[("PATH", &path), ("FAKE_XCODE", self.remotes.as_os_str())],
        )
    }

    fn export_xcode(&self, skills: &[(&str, &str)], version: &str) {
        let export = self.remotes.join("export");
        if export.exists() {
            fs::remove_dir_all(&export).unwrap();
        }
        fs::create_dir_all(&export).unwrap();
        fs::write(self.remotes.join("version"), version).unwrap();
        for (name, body) in skills {
            write_skill(&export.join(name), name, body);
        }
    }
}

#[test]
fn xcode_all_refreshes_and_records_provenance() {
    let env = Env::new();
    env.write_manifest("[xcode]\nos = ['macos', 'linux']\nskills = '*'\n");
    env.export_xcode(&[("alpha", "v1")], "Xcode 27\nBuild version A");
    assert!(env.xcode(&["sync"]).status.success());
    let old = fs::read_link(env.codex("alpha")).unwrap();
    assert!(body(&env.claude("alpha")).contains("v1"));
    assert!(
        fs::read_to_string(env.config("skills.lock"))
            .unwrap()
            .contains("Build version A")
    );
    assert!(env.xcode(&["sync"]).status.success());
    assert_eq!(old, fs::read_link(env.codex("alpha")).unwrap());
    env.export_xcode(
        &[("alpha", "v2"), ("beta", "new")],
        "Xcode 27.1\nBuild version B",
    );
    assert!(env.xcode(&["sync"]).status.success());
    assert!(body(&env.codex("alpha")).contains("v2"));
    assert!(env.codex("beta").exists());
    assert!(!old.exists());
    let list = env.ok(&["list"]);
    assert!(list.contains("Xcode 27.1"));
    env.export_xcode(&[("beta", "new")], "Xcode 27.2");
    assert!(env.xcode(&["sync"]).status.success());
    assert!(!env.codex("alpha").exists());
}

#[test]
fn xcode_selected_missing_and_failed_exports_preserve_install() {
    let env = Env::new();
    env.write_manifest(
        "[xcode]\nos = ['macos', 'linux']\nskills = ['alpha']\nagents = ['codex']\n",
    );
    env.export_xcode(&[("alpha", "v1"), ("beta", "other")], "Xcode 27");
    assert!(env.xcode(&["sync"]).status.success());
    assert!(!env.codex("beta").exists());
    assert!(!env.claude("alpha").exists());
    let lock = fs::read(env.config("skills.lock")).unwrap();
    let target = fs::read_link(env.codex("alpha")).unwrap();
    env.export_xcode(&[("renamed", "v2")], "Xcode 28");
    assert!(!env.xcode(&["sync"]).status.success());
    assert_eq!(lock, fs::read(env.config("skills.lock")).unwrap());
    assert_eq!(target, fs::read_link(env.codex("alpha")).unwrap());
    fs::write(env.remotes.join("fail"), "").unwrap();
    assert!(!env.xcode(&["sync"]).status.success());
    assert_eq!(lock, fs::read(env.config("skills.lock")).unwrap());
    assert!(body(&env.codex("alpha")).contains("v1"));
    env.write_manifest("[xcode]\nos = ['macos', 'linux']\nskills = []\n");
    // Removing the selection also works when Xcode is unavailable.
    assert!(env.xcode(&["sync"]).status.success());
    assert!(!env.codex("alpha").exists());
}

#[test]
fn xcode_dry_run_and_locked_do_not_invoke_exporter() {
    let env = Env::new();
    env.write_manifest("[xcode]\nos = ['macos', 'linux']\nskills = ['alpha']\n");
    env.export_xcode(&[("alpha", "v1")], "Xcode 27");
    assert!(env.xcode(&["sync", "--dry-run"]).status.success());
    assert!(!env.remotes.join("called").exists());
    assert!(!env.config("skills.lock").exists());
    assert!(!env.home.join(".local/share/skl/store").exists());
    assert!(!env.home.join(".local/share/skl/state.toml").exists());
    assert!(!env.xcode(&["sync", "--locked"]).status.success());
    assert!(!env.remotes.join("called").exists());
    assert!(env.xcode(&["sync"]).status.success());
    fs::remove_file(env.remotes.join("called")).unwrap();
    let lock = fs::read(env.config("skills.lock")).unwrap();
    assert!(env.xcode(&["sync", "--locked"]).status.success());
    assert!(!env.remotes.join("called").exists());
    assert_eq!(lock, fs::read(env.config("skills.lock")).unwrap());
    env.write_manifest("[xcode]\nos = ['macos', 'linux']\nskills = ['alpha', 'beta']\n");
    assert!(!env.xcode(&["sync", "--locked"]).status.success());
    assert_eq!(lock, fs::read(env.config("skills.lock")).unwrap());
}

#[test]
fn xcode_conflicts_are_backed_up_only_with_force() {
    let env = Env::new();
    env.write_manifest("[xcode]\nos = ['macos', 'linux']\nskills = '*'\n");
    env.export_xcode(&[("alpha", "exported")], "Xcode 27");
    write_skill(&env.codex("alpha"), "alpha", "user content");
    assert!(!env.xcode(&["sync"]).status.success());
    assert!(body(&env.codex("alpha")).contains("user content"));
    assert!(env.xcode(&["sync", "--force"]).status.success());
    assert!(body(&env.codex("alpha")).contains("exported"));
    let backups = env.home.join(".local/share/skl/backup");
    let backup = fs::read_dir(backups)
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    assert!(body(&backup.join("alpha")).contains("user content"));
}

#[test]
fn xcode_invalid_exports_preserve_previous_content() {
    let env = Env::new();
    env.write_manifest("[xcode]\nos = ['macos', 'linux']\nskills = '*'\n");
    env.export_xcode(&[("alpha", "v1")], "Xcode 27");
    assert!(env.xcode(&["sync"]).status.success());
    let lock = fs::read(env.config("skills.lock")).unwrap();
    let skill = env.remotes.join("export/alpha/SKILL.md");
    fs::remove_file(&skill).unwrap();
    std::os::unix::fs::symlink(env.config("skills.toml"), &skill).unwrap();
    assert!(!env.xcode(&["sync"]).status.success());
    assert_eq!(lock, fs::read(env.config("skills.lock")).unwrap());
    assert!(body(&env.codex("alpha")).contains("v1"));
}

#[test]
fn xcode_profiles_skip_export_and_preserve_lock() {
    let env = Env::new();
    env.write_manifest("[xcode]\nos = ['macos', 'linux']\nskills = '*'\nprofiles = ['mac']\n");
    env.export_xcode(&[("alpha", "v1")], "Xcode 27");
    assert!(env.xcode(&["sync"]).status.success());
    assert!(!env.remotes.join("called").exists());
    assert!(env.xcode(&["--profile", "mac", "sync"]).status.success());
    let lock = fs::read(env.config("skills.lock")).unwrap();
    fs::remove_file(env.remotes.join("called")).unwrap();
    assert!(env.xcode(&["sync"]).status.success());
    assert!(!env.remotes.join("called").exists());
    assert!(!env.codex("alpha").exists());
    assert_eq!(lock, fs::read(env.config("skills.lock")).unwrap());
}

#[test]
fn xcode_targeted_update_only_refreshes_requested_skill() {
    let env = Env::new();
    env.write_manifest("[xcode]\nos = ['macos', 'linux']\nskills = '*'\n");
    env.export_xcode(&[("alpha", "v1"), ("beta", "v1")], "Xcode 27");
    assert!(env.xcode(&["sync"]).status.success());
    env.export_xcode(&[("alpha", "v2"), ("gamma", "new")], "Xcode 28");
    assert!(env.xcode(&["update", "alpha"]).status.success());
    assert!(body(&env.codex("alpha")).contains("v2"));
    assert!(body(&env.codex("beta")).contains("v1"));
    assert!(!env.codex("gamma").exists());
    let lock = fs::read(env.config("skills.lock")).unwrap();
    let count = env.store_commits();
    env.export_xcode(&[("alpha", "v3")], "Xcode 29");
    assert!(!env.xcode(&["update", "unknown"]).status.success());
    assert_eq!(lock, fs::read(env.config("skills.lock")).unwrap());
    assert_eq!(count, env.store_commits());
}

#[test]
fn xcode_rejects_invalid_config_and_duplicate_sources() {
    let env = Env::new();
    for config in [
        "skills = true",
        "skills = ['../bad']",
        "skills = '*'\npath = 'ignored'",
        "skills = '*'\nprofiles = []",
    ] {
        env.write_manifest(&format!("[xcode]\nos = ['macos', 'linux']\n{config}\n"));
        assert!(!env.xcode(&["sync"]).status.success());
        assert!(!env.remotes.join("called").exists());
    }
    env.export_xcode(&[("alpha", "exported")], "Xcode 27");
    env.write_manifest(
        "[skills]\nalpha = { path = '~/local' }\n[xcode]\nos = ['macos', 'linux']\nskills = '*'\n",
    );
    let output = env.xcode(&["sync"]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("conflicts with [skills]"));
    assert_eq!(env.store_commits(), 0);
    assert!(!env.codex("alpha").exists());
}

#[test]
fn xcode_frontmatter_order_does_not_change_snapshot() {
    use std::os::unix::fs::PermissionsExt;
    let env = Env::new();
    env.write_manifest("[xcode]\nos = ['macos', 'linux']\nskills = '*'\n");
    env.export_xcode(&[("alpha", "body")], "Xcode 27");
    let skill = env.remotes.join("export/alpha/SKILL.md");
    fs::write(
        &skill,
        "---\nname: alpha\ndescription: |\n  First line\n  Second line\neffort: high\n---\nbody\n",
    )
    .unwrap();
    fs::set_permissions(&skill, fs::Permissions::from_mode(0o444)).unwrap();
    assert!(env.xcode(&["sync"]).status.success());
    let lock = fs::read(env.config("skills.lock")).unwrap();
    let target = fs::read_link(env.codex("alpha")).unwrap();
    fs::set_permissions(&skill, fs::Permissions::from_mode(0o644)).unwrap();
    fs::write(
        &skill,
        "---\ndescription: |\n  First line\n  Second line\neffort: high\nname: alpha\n---\nbody\n",
    )
    .unwrap();
    fs::set_permissions(&skill, fs::Permissions::from_mode(0o444)).unwrap();
    assert!(env.xcode(&["sync"]).status.success());
    assert_eq!(lock, fs::read(env.config("skills.lock")).unwrap());
    assert_eq!(target, fs::read_link(env.codex("alpha")).unwrap());
    assert!(body(&env.codex("alpha")).contains("description: |\n  First line\n  Second line\n"));
}

#[test]
fn xcode_locked_requires_verified_wildcard_coverage() {
    let env = Env::new();
    env.export_xcode(&[("alpha", "v1"), ("beta", "v1")], "Xcode 27");
    env.write_manifest("[xcode]\nos = ['macos', 'linux']\nskills = ['alpha']\n");
    assert!(env.xcode(&["sync"]).status.success());
    let lock = fs::read(env.config("skills.lock")).unwrap();
    env.write_manifest("[xcode]\nos = ['macos', 'linux']\nskills = '*'\n");
    fs::remove_file(env.remotes.join("called")).unwrap();
    let result = env.xcode(&["sync", "--locked"]);
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("not cached for this selection"));
    assert!(!env.remotes.join("called").exists());
    assert_eq!(lock, fs::read(env.config("skills.lock")).unwrap());
    assert!(env.codex("alpha").exists());
    assert!(!env.codex("beta").exists());

    // A failed refresh cannot turn a subset into verified wildcard coverage.
    fs::write(env.remotes.join("fail"), "").unwrap();
    assert!(!env.xcode(&["sync"]).status.success());
    assert!(!env.xcode(&["sync", "--locked"]).status.success());
    fs::remove_file(env.remotes.join("fail")).unwrap();
    assert!(env.xcode(&["sync"]).status.success());
    assert!(env.codex("beta").exists());
    let complete = fs::read_to_string(env.config("skills.lock")).unwrap();
    assert!(complete.contains("xcode_all_complete = true"));
    assert!(env.xcode(&["sync", "--locked"]).status.success());

    // Old lockfiles without coverage metadata must be refreshed once.
    fs::write(
        env.config("skills.lock"),
        complete.replace("xcode_all_complete = true\n", ""),
    )
    .unwrap();
    assert!(!env.xcode(&["sync", "--locked"]).status.success());
}

#[test]
fn xcode_targeted_import_does_not_claim_complete_coverage() {
    let env = Env::new();
    env.export_xcode(&[("alpha", "v1"), ("beta", "v1")], "Xcode 27");
    env.write_manifest("[xcode]\nos = ['macos', 'linux']\nskills = '*'\n");
    assert!(env.xcode(&["update", "alpha"]).status.success());
    assert!(!env.xcode(&["sync", "--locked"]).status.success());
    assert!(!env.codex("beta").exists());
    assert!(env.xcode(&["sync"]).status.success());
    assert!(env.xcode(&["sync", "--locked"]).status.success());

    // A later targeted update also invalidates coverage if a new export is skipped.
    env.export_xcode(
        &[("alpha", "v2"), ("beta", "v1"), ("gamma", "v1")],
        "Xcode 28",
    );
    assert!(env.xcode(&["update", "alpha"]).status.success());
    assert!(!env.xcode(&["sync", "--locked"]).status.success());
}

#[test]
fn xcode_empty_export_has_complete_coverage() {
    let env = Env::new();
    env.export_xcode(&[], "Xcode 27");
    env.write_manifest("[xcode]\nos = ['macos', 'linux']\nskills = '*'\n");
    assert!(env.xcode(&["sync"]).status.success());
    assert!(env.xcode(&["sync", "--locked"]).status.success());
    env.ok(&["list"]);
}

fn other_os() -> &'static str {
    if std::env::consts::OS == "macos" {
        "linux"
    } else {
        "macos"
    }
}

#[test]
fn os_filters_skip_git_and_local_skills_and_preserve_lock() {
    let env = Env::new();
    let url = env.repo("notes", &[("", "notes")]);
    write_skill(&env.home.join("local"), "local", "local body");
    let manifest = |os| {
        format!(
            "[skills]\nnotes = {{ source = '{url}', os = ['{os}'], profiles = ['work'] }}\nlocal = {{ path = '~/local', os = ['{os}'], profiles = ['work'] }}\n"
        )
    };
    env.write_manifest(&manifest(std::env::consts::OS));
    env.ok(&["sync"]);
    assert!(!env.codex("notes").exists());
    env.ok(&["--profile", "work", "sync"]);
    assert!(env.codex("notes").exists());
    assert!(env.codex("local").exists());
    let lock = fs::read(env.config("skills.lock")).unwrap();
    env.write_manifest(&manifest(other_os()));
    let preview = env.ok(&["--profile", "work", "sync", "--dry-run"]);
    assert!(preview.contains("links removed"));
    assert!(env.codex("notes").exists());
    env.ok(&["--profile", "work", "sync", "--locked"]);
    assert!(!env.codex("notes").exists());
    assert!(!env.codex("local").exists());
    assert_eq!(lock, fs::read(env.config("skills.lock")).unwrap());
    assert!(
        env.ok(&["--profile", "work", "list"])
            .contains(&format!("only on {}", other_os()))
    );
    assert!(
        env.fails(&["--profile", "work", "update", "notes"])
            .contains("not enabled on")
    );

    // Excluded sources are never resolved, even when they don't exist locally.
    env.write_manifest(&format!(
        "[skills]\nmissing = {{ source = 'file:///does-not-exist', os = ['{}'] }}\n",
        other_os()
    ));
    env.ok(&["sync"]);
}

#[test]
fn xcode_os_filter_skips_export_and_preserves_coverage() {
    let env = Env::new();
    env.export_xcode(&[("alpha", "v1")], "Xcode 27");
    env.write_manifest("[xcode]\nos = ['macos', 'linux']\nskills = '*'\n");
    assert!(env.xcode(&["sync"]).status.success());
    let lock = fs::read(env.config("skills.lock")).unwrap();
    fs::remove_file(env.remotes.join("called")).unwrap();
    env.write_manifest(&format!("[xcode]\nos = ['{}']\nskills = '*'\n", other_os()));
    assert!(env.xcode(&["sync", "--locked"]).status.success());
    assert!(!env.remotes.join("called").exists());
    assert!(!env.codex("alpha").exists());
    assert_eq!(lock, fs::read(env.config("skills.lock")).unwrap());
    assert!(!env.xcode(&["update", "alpha"]).status.success());
    assert!(!env.remotes.join("called").exists());
    assert!(
        env.ok(&["list"])
            .contains(&format!("only on {}", other_os()))
    );
}

#[test]
fn xcode_defaults_to_macos() {
    let env = Env::new();
    env.export_xcode(&[("alpha", "v1")], "Xcode 27");
    env.write_manifest("[xcode]\nskills = '*'\n");
    assert!(env.xcode(&["sync"]).status.success());
    assert_eq!(
        env.remotes.join("called").exists(),
        cfg!(target_os = "macos")
    );
    assert_eq!(env.codex("alpha").exists(), cfg!(target_os = "macos"));
    env.ok(&["list"]);
}

#[test]
fn rejects_invalid_os_filters() {
    let env = Env::new();
    for value in ["[]", "'macos'", "['darwin']", "[7]"] {
        env.write_manifest(&format!(
            "[skills]\nnotes = {{ path = '~/local', os = {value} }}\n"
        ));
        env.fails(&["sync"]);
        env.write_manifest(&format!("[xcode]\nskills = '*'\nos = {value}\n"));
        assert!(!env.xcode(&["sync"]).status.success());
        assert!(!env.remotes.join("called").exists());
    }
}

#[path = "../examples/prepare_release.rs"]
mod release_changelog;

#[test]
fn release_promotes_unreleased_and_is_repeatable() {
    let original = "# Changelog\n\n## [Unreleased]\n\n### Fixed\n\n- A fix.\n\n## [0.1.0] - 2026-10-14\n\n- Initial release.\n";
    let (updated, notes) = release_changelog::prepare(original, "v0.2.0", "2026-10-15").unwrap();
    assert_eq!(notes, "### Fixed\n\n- A fix.\n");
    assert_eq!(
        updated,
        original.replace(
            "## [Unreleased]",
            "## [Unreleased]\n\n## [0.2.0] - 2026-10-15"
        )
    );
    assert_eq!(
        release_changelog::prepare(&updated, "v0.2.0", "2026-10-16").unwrap(),
        (updated, notes)
    );
}

#[test]
fn release_uses_exact_existing_version_without_consuming_unreleased() {
    let original = "## [Unreleased]\n\n- Future.\n\n## [0.2.10]\n\n- Other.\n\n## [0.2.1] - 2026-10-14\n\n- Released.\n";
    let (updated, notes) = release_changelog::prepare(original, "v0.2.1", "2026-10-15").unwrap();
    assert_eq!(updated, original);
    assert_eq!(notes, "- Released.\n");
}

#[test]
fn release_rejects_missing_or_empty_notes() {
    for original in [
        "# Changelog\n",
        "## [Unreleased]\n\n",
        "## [Unreleased]\n\n### Fixed\n\n## [0.1.0]\n\n- Old.\n",
        "## [Unreleased]\n\n- Future.\n\n## [0.2.0]\n",
    ] {
        assert!(release_changelog::prepare(original, "v0.2.0", "2026-10-15").is_err());
    }
}

#[test]
fn release_saves_to_advanced_branch_and_reruns_without_duplicate_commits() {
    let tmp = TempDir::new().unwrap();
    let remote = tmp.path().join("remote.git");
    let checkout = tmp.path().join("checkout");
    fs::create_dir(&remote).unwrap();
    git(&remote, &["init", "--bare", "--initial-branch=main"]);
    git(tmp.path(), &["clone", remote.to_str().unwrap(), "checkout"]);
    let original = "# Changelog\n\n## [Unreleased]\n\n- A fix.\n";
    fs::write(checkout.join("CHANGELOG.md"), original).unwrap();
    let tagged = commit(&checkout, "release source");
    git(&checkout, &["tag", "v0.2.0"]);
    fs::write(checkout.join("new-work.txt"), "preserve this").unwrap();
    commit(&checkout, "work after the tag");
    git(&checkout, &["push", "origin", "main"]);

    // Exercise the actual workflow's save commands against a disposable remote.
    let workflow = include_str!("../.github/workflows/release.yml");
    let save_step = workflow
        .split("      - name: Save changelog on the default branch\n")
        .nth(1)
        .unwrap();
    let script = save_step
        .split("        run: |\n")
        .nth(1)
        .unwrap()
        .lines()
        .map(|line| line.strip_prefix("          ").unwrap_or(line))
        .collect::<Vec<_>>()
        .join("\n");
    let mut saved = String::new();
    for date in ["2026-10-15", "2026-10-16"] {
        git(&checkout, &["checkout", "--detach", &tagged]);
        let (updated, _) = release_changelog::prepare(original, "v0.2.0", date).unwrap();
        fs::write(checkout.join("CHANGELOG.md"), updated).unwrap();
        let output = Command::new("bash")
            .args(["-c", &script])
            .current_dir(&checkout)
            .env("HOME", tmp.path())
            .env("TAG", "v0.2.0")
            .env("DEFAULT_BRANCH", "main")
            .env("GIT_CONFIG_COUNT", "1")
            .env("GIT_CONFIG_KEY_0", "commit.gpgsign")
            .env("GIT_CONFIG_VALUE_0", "false")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let head = git(&remote, &["rev-parse", "main"]);
        if saved.is_empty() {
            saved = head;
        } else {
            assert_eq!(head, saved);
        }
        assert_eq!(
            fs::read_to_string(checkout.join("new-work.txt")).unwrap(),
            "preserve this"
        );
        assert_eq!(git(&checkout, &["rev-parse", "v0.2.0"]), tagged);
    }
}

#[test]
fn agent_defaults_preserve_manifest_and_apply_on_sync() {
    let env = Env::new();
    write_skill(&env.home.join("local"), "demo", "v1");
    env.write_manifest("# keep this comment\n[skills]\ndemo = { path = '~/local' }\nonly-codex = { path = '~/local', agents = ['codex'] }\n");
    env.ok(&["sync"]);
    let original = fs::read_to_string(env.config("skills.toml")).unwrap();
    assert!(
        env.fails(&["agents", "set", "cursor", "typo-agent"])
            .contains("unknown agent")
    );
    assert_eq!(
        fs::read_to_string(env.config("skills.toml")).unwrap(),
        original
    );
    env.ok(&["agents", "set", "cursor", "opencode", "cursor"]);
    assert!(env.claude("demo").is_symlink()); // Setting alone does not sync.
    let text = fs::read_to_string(env.config("skills.toml")).unwrap();
    assert!(text.contains("# keep this comment"));
    assert!(text.contains("agents = ['codex']"));
    assert!(env.ok(&["agents"]).contains("cursor, opencode"));
    env.ok(&["sync"]);
    assert!(!env.claude("demo").exists());
    assert!(!env.codex("demo").exists());
    assert!(env.codex("only-codex").is_symlink());
    assert!(env.home.join(".cursor/skills/demo").is_symlink());
    assert!(env.home.join(".config/opencode/skills/demo").is_symlink());
    env.ok(&["agents", "reset"]);
    env.ok(&["sync"]);
    assert!(env.claude("demo").is_symlink());
    assert!(!env.home.join(".cursor/skills/demo").exists());
}

#[test]
fn all_named_agents_sync_and_shared_directories_remain_owned() {
    let env = Env::new();
    let listing = env.ok(&["agents", "list"]);
    assert!(!env.home.join(".local/share/skl").exists());
    let rows: Vec<_> = listing
        .lines()
        .map(|line| {
            let mut parts = line.split_whitespace();
            (parts.next().unwrap(), parts.next().unwrap())
        })
        .collect();
    assert!(rows.len() >= 70);
    write_skill(&env.home.join("local"), "demo", "v1");
    env.write_manifest("[skills]\ndemo = { path = '~/local' }\n");
    let mut args = vec!["agents", "set"];
    args.extend(rows.iter().map(|(name, _)| *name));
    env.ok(&args);
    env.ok(&["sync"]);
    for (_, path) in &rows {
        assert!(
            env.home
                .join(path.strip_prefix("~/").unwrap())
                .join("demo")
                .is_symlink(),
            "{path}"
        );
    }
    env.ok(&["agents", "set", "cline", "codex", "warp"]);
    env.ok(&["sync"]);
    assert!(env.codex("demo").is_symlink());
    env.ok(&["agents", "set", "codex"]);
    env.ok(&["sync"]);
    assert!(env.codex("demo").is_symlink());
    assert!(!env.claude("demo").exists());
}

#[test]
fn agent_defaults_support_custom_paths_and_symlinked_manifest() {
    use std::os::unix::fs::symlink;
    let env = Env::new();
    env.write_manifest("# dotfiles\n[skills]\n");
    let dotfile = env.home.join("dotfile.toml");
    fs::rename(env.config("skills.toml"), &dotfile).unwrap();
    symlink(&dotfile, env.config("skills.toml")).unwrap();
    env.ok(&["agents", "set", "~/custom/skills"]);
    assert!(env.config("skills.toml").is_symlink());
    assert!(
        fs::read_to_string(dotfile)
            .unwrap()
            .contains("~/custom/skills")
    );
    assert!(env.ok(&["agents"]).contains("~/custom/skills"));
}

#[test]
fn xdg_agent_paths_resolve_without_changing_other_harnesses() {
    let env = Env::new();
    let config = env.home.join("config-override");
    let out = env.skl_with_env(
        &["agents", "list"],
        &[("XDG_CONFIG_HOME", config.as_os_str())],
    );
    assert!(out.status.success());
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(text.lines().any(|l| l.starts_with("opencode ") && l.ends_with("~/config-override/opencode/skills")));
    assert!(
        text.lines()
            .any(|l| l.starts_with("codex ") && l.ends_with("~/.agents/skills"))
    );
    let out = env.skl_with_env(
        &["agents", "list"],
        &[("XDG_CONFIG_HOME", std::ffi::OsStr::new("relative"))],
    );
    assert!(
        String::from_utf8(out.stdout)
            .unwrap()
            .lines()
            .any(|l| l.starts_with("opencode ") && l.ends_with("~/.config/opencode/skills"))
    );
}
