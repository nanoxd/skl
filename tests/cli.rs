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
            .contains("exactly one of `source`, `path` or `command`")
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
            &[("PATH", &path), ("DEVELOPER_DIR", self.remotes.as_os_str())],
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
fn xcode_refresh_tolerates_disappearing_temporary_files() {
    let env = Env::new();
    env.write_manifest("[xcode]\nskills = '*'\nos = ['macos', 'linux']\n");
    env.export_xcode(&[("alpha", "v1")], "Xcode 27");
    assert!(env.xcode(&["sync"]).status.success());

    env.write_manifest("[xcode]\nskills = ['alpha']\nos = ['macos', 'linux']\n");
    env.export_xcode(&[("alpha", "v2")], "Xcode 28");
    fs::write(env.remotes.join("temporary-files"), "").unwrap();
    let out = env.xcode(&["sync"]);
    assert!(out.status.success(), "{out:?}");
    assert!(body(&env.codex("alpha")).contains("v2"));
    assert!(body(&env.claude("alpha")).contains("v2"));
    assert!(env.xcode(&["sync", "--locked"]).status.success());
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
    assert!(String::from_utf8_lossy(&output.stderr).contains("conflicts with another declaration"));
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
    let mut uncovered: toml::Table = toml::from_str(&complete).unwrap();
    let xcode = uncovered["sources"]["xcode"].as_table_mut().unwrap();
    assert!(xcode["members"].get("beta").is_some());
    assert!(xcode["snapshots"].get("beta").is_some());
    xcode.remove("members");
    assert!(env.xcode(&["sync", "--locked"]).status.success());

    // Lockfiles without coverage metadata must be refreshed once.
    fs::write(
        env.config("skills.lock"),
        toml::to_string(&uncovered).unwrap(),
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

#[path = "../tools/prepare_release.rs"]
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
fn release_tag_must_match_the_package_version() {
    assert!(release_changelog::check_version("v0.4.0", "0.4.0").is_ok());
    assert!(release_changelog::check_version("v0.4.0-rc.1", "0.4.0-rc.1").is_ok());
    for tag in ["v0.3.0", "0.4.0", "v0.4.0-rc.1", "v0.4"] {
        assert!(release_changelog::check_version(tag, "0.4.0").is_err());
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
        .unwrap()
        .split("      - name:")
        .next()
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

#[test]
fn saved_update_plan_pins_reviewed_commits_and_selection() {
    let e = Env::new();
    let url = e.repo("plans", &[("a", "a"), ("b", "b")]);
    e.write_manifest(&format!(
        "[skills]\na = {{source = '{url}'}}\nb = {{source = '{url}'}}\n"
    ));
    e.ok(&["sync"]);
    let initial_lock = fs::read(e.config("skills.lock")).unwrap();
    let initial_state = fs::read(e.home.join(".local/share/skl/state.toml")).unwrap();
    write_skill(&e.remote("plans").join("a"), "a", "reviewed");
    write_skill(&e.remote("plans").join("b"), "b", "unselected");
    let reviewed = commit(&e.remote("plans"), "reviewed update");
    e.ok(&["update", "a", "--plan", "review.toml"]);
    let plan = fs::read_to_string(e.home.join("review.toml")).unwrap();
    assert!(plan.contains(&reviewed));
    assert!(plan.contains("+reviewed"));
    assert_eq!(fs::read(e.config("skills.lock")).unwrap(), initial_lock);
    assert_eq!(
        fs::read(e.home.join(".local/share/skl/state.toml")).unwrap(),
        initial_state
    );
    assert_eq!(e.store_commits(), 1);
    write_skill(&e.remote("plans").join("a"), "a", "unreviewed");
    commit(&e.remote("plans"), "later change");
    e.ok(&["update", "--apply-plan", "review.toml", "--dry-run"]);
    assert!(body(&e.codex("a")).contains("v1"));
    e.ok(&["update", "--apply-plan", "review.toml"]);
    assert!(body(&e.codex("a")).contains("reviewed"));
    assert!(!body(&e.codex("a")).contains("unreviewed"));
    assert!(body(&e.codex("b")).contains("v1"));
    assert!(
        e.fails(&["update", "--apply-plan", "review.toml"])
            .contains("stale")
    );
}

#[test]
fn saved_update_plan_is_read_only_and_rejects_stale_or_edited_context() {
    let e = Env::new();
    let url = e.repo("plans", &[("a", "a")]);
    let manifest = format!("[skills]\na = {{source = '{url}'}}\n");
    e.write_manifest(&manifest);
    e.ok(&["update", "--plan", "review.toml"]);
    assert!(!e.home.join(".local/share/skl").exists());
    assert!(!e.config("skills.lock").exists());
    let original = fs::read_to_string(e.home.join("review.toml")).unwrap();
    e.write_manifest(&format!("{manifest}# changed\n"));
    assert!(
        e.fails(&["update", "--apply-plan", "review.toml"])
            .contains("stale")
    );
    e.write_manifest(&manifest);
    assert!(
        e.fails(&["update", "--profile", "work", "--apply-plan", "review.toml"])
            .contains("stale")
    );
    fs::write(
        e.home.join("review.toml"),
        original.replacen("version = 3", "version = 900", 1),
    )
    .unwrap();
    assert!(
        e.fails(&["update", "--apply-plan", "review.toml"])
            .contains("unsupported")
    );
    fs::write(
        e.home.join("review.toml"),
        original.replace("+v1", "+tampered"),
    )
    .unwrap();
    assert!(
        e.fails(&["update", "--apply-plan", "review.toml"])
            .contains("checksum")
    );
    fs::write(e.home.join("review.toml"), "not toml").unwrap();
    assert!(
        e.fails(&["update", "--apply-plan", "review.toml"])
            .contains("invalid update plan")
    );
}

#[test]
fn saved_update_plan_does_not_overwrite_or_write_managed_locations() {
    let e = Env::new();
    e.write_manifest("[skills]\n");
    e.ok(&["update", "--plan", "review.toml"]);
    let before = fs::read(e.home.join("review.toml")).unwrap();
    e.fails(&["update", "--plan", "review.toml"]);
    assert_eq!(fs::read(e.home.join("review.toml")).unwrap(), before);
    e.fails(&["update", "--plan", ".config/skl/skills.lock"]);
    assert!(!e.config("skills.lock").exists());
    std::os::unix::fs::symlink(e.config("skills.toml"), e.home.join("alias")).unwrap();
    e.fails(&["update", "--plan", "alias"]);
    assert_eq!(
        fs::read_to_string(e.config("skills.toml")).unwrap(),
        "[skills]\n"
    );
    e.fails(&["update", "--plan", "other.toml", "--dry-run"]);
    e.fails(&["update", "a", "--apply-plan", "review.toml"]);
    e.fails(&[
        "update",
        "--plan",
        "other.toml",
        "--apply-plan",
        "review.toml",
    ]);
}

#[test]
fn saved_update_plan_fails_without_artifact_for_missing_skill_or_xcode() {
    let e = Env::new();
    e.write_manifest("[skills]\na = {source = 'file:///nonexistent-plan-test'}\n");
    e.fails(&["update", "--plan", "review.toml"]);
    assert!(!e.home.join("review.toml").exists());
    assert!(!e.home.join(".local/share/skl").exists());
    e.write_manifest("[xcode]\nskills = '*'\nos = ['macos', 'linux']\n");
    fs::write(e.remotes.join("fail"), "unavailable").unwrap();
    let output = e.xcode(&["update", "--plan", "review.toml"]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("Xcode"));
    assert!(!e.home.join("review.toml").exists());
}

#[test]
fn saved_update_plan_partial_fetch_failure_keeps_previous_install() {
    let e = Env::new();
    let a = e.repo("a", &[("", "a")]);
    let b = e.repo("b", &[("", "b")]);
    e.write_manifest(&format!(
        "[skills]\na = {{source = '{a}'}}\nb = {{source = '{b}'}}\n"
    ));
    e.ok(&["sync"]);
    for name in ["a", "b"] {
        write_skill(&e.remote(name), name, "reviewed");
        commit(&e.remote(name), "update");
    }
    e.ok(&["update", "--plan", "review.toml"]);
    fs::remove_dir_all(e.remote("b")).unwrap();
    e.fails(&["update", "--apply-plan", "review.toml"]);
    assert!(body(&e.codex("a")).contains("reviewed"));
    assert!(body(&e.codex("b")).contains("v1"));
    e.ok(&["sync", "--locked"]);
}

#[test]
fn saved_update_plan_rejects_changed_resolved_agent_destinations() {
    let e = Env::new();
    e.write_manifest("agents = ['opencode']\n[skills]\n");
    // Include a local skill to exercise resolved per-skill destinations.
    write_skill(&e.home.join("local"), "local", "v1");
    e.write_manifest("agents = ['opencode']\n[skills]\nlocal = {path = '~/local'}\n");
    e.ok(&["update", "--plan", "review.toml"]);
    let different_xdg = e.home.join("other-config");
    let config = e.config("");
    let output = e.skl_with_env(
        &["update", "--apply-plan", "review.toml"],
        &[
            ("SKL_CONFIG_DIR", config.as_os_str()),
            ("XDG_CONFIG_HOME", different_xdg.as_os_str()),
        ],
    );
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("stale"));
}

#[test]
fn info_is_read_only_and_explains_filters_and_missing_content() {
    let env = Env::new();
    let other_os = if cfg!(target_os = "macos") {
        "linux"
    } else {
        "macos"
    };
    env.write_manifest(&format!(
        "[skills]\nnotes = {{ path = '~/notes', profiles = ['work'], os = ['{other_os}'] }}\n"
    ));
    let before = fs::read(env.config("skills.toml")).unwrap();
    let out = env.ok(&["info", "notes"]);
    assert!(out.contains("Origin: local"));
    assert!(out.contains("missing SKILL.md"));
    assert!(out.contains("Selection: excluded: OS"));
    assert!(out.contains("active profiles do not match [work]"));
    assert!(out.contains("~/.claude/skills/notes — missing"));
    assert!(!env.home.join(".local").exists());
    assert!(!env.config("skills.lock").exists());
    assert_eq!(fs::read(env.config("skills.toml")).unwrap(), before);
    let out = env.ok(&["--profile", "work", "info", "notes"]);
    assert!(out.contains("Active profiles: work (--profile)"));
    assert!(!out.contains("active profiles do not match"));
}

#[test]
fn info_reports_git_pin_stale_lock_and_actual_ownership() {
    let env = Env::new();
    let source = env.repo("info", &[("nested/notes", "notes")]);
    env.write_manifest(&format!("[skills]\nnotes = '{source}'\n"));
    env.ok(&["sync"]);
    let out = env.ok(&["info", "notes"]);
    assert!(out.contains("Pinned commit:"));
    assert!(out.contains("Pinned subdirectory: nested/notes"));
    assert!(out.contains("Lock: matches manifest"));
    assert!(out.contains("installed (owned)"));
    let original = fs::read_link(env.claude("notes")).unwrap();
    fs::remove_file(env.claude("notes")).unwrap();
    let other = env.home.join("other");
    write_skill(&other, "notes", "user");
    std::os::unix::fs::symlink(&other, env.claude("notes")).unwrap();
    let out = env.ok(&["info", "notes"]);
    assert!(out.contains("~/.claude/skills/notes — unmanaged"));
    assert!(out.contains("Recorded target:"));
    assert_eq!(fs::read_link(env.claude("notes")).unwrap(), other);
    env.write_manifest(&format!(
        "[skills]\nnotes = {{ source = '{source}', ref = 'changed' }}\n"
    ));
    assert!(env.ok(&["info", "notes"]).contains("Lock: stale"));
    assert!(original.exists());
}

#[test]
fn info_handles_unmanaged_unknown_and_unexported_xcode() {
    let env = Env::new();
    write_skill(&env.claude("external"), "external", "user");
    let out = env.ok(&["info", "external"]);
    assert!(out.contains("Origin: unmanaged"));
    assert!(out.contains("Selection: not in manifest"));
    assert!(!env.home.join(".local").exists());
    assert!(env.fails(&["info", "unknown"]).contains("unknown skill"));
    assert!(
        env.fails(&["info", "../external"])
            .contains("not a valid skill name")
    );
    env.write_manifest("[xcode]\nskills = ['apple']\n");
    let out = env.ok(&["info", "apple"]);
    assert!(out.contains("Origin: app"));
    assert!(out.contains("Source: xcode"));
    assert!(out.contains("App export: not exported"));
    assert!(!env.home.join(".local").exists());
}

#[test]
fn info_without_lock_does_not_fetch_and_rejects_corrupt_metadata() {
    let env = Env::new();
    env.write_manifest("[skills]\nnotes = 'nonexistent/never-fetch'\n");
    assert!(env.ok(&["info", "notes"]).contains("Lock: not locked"));
    assert!(!env.home.join(".local").exists());
    fs::write(env.config("skills.lock"), "not toml!").unwrap();
    assert!(env.fails(&["info", "notes"]).contains("invalid lockfile"));
}

#[test]
fn info_distinguishes_owned_wrong_target_broken_and_user_store_links() {
    let env = Env::new();
    let old = env.home.join("old");
    let new = env.home.join("new");
    write_skill(&old, "notes", "old");
    write_skill(&new, "notes", "new");
    env.write_manifest("[skills]\nnotes = { path = '~/old' }\n");
    env.ok(&["sync"]);
    env.write_manifest("[skills]\nnotes = { path = '~/new' }\n");
    assert!(
        env.ok(&["info", "notes"])
            .contains("different target (owned)")
    );
    fs::remove_file(old.join("SKILL.md")).unwrap();
    assert!(env.ok(&["info", "notes"]).contains("broken (owned)"));
    fs::remove_file(env.home.join(".local/share/skl/state.toml")).unwrap();
    assert!(env.ok(&["info", "notes"]).contains("unmanaged"));
}

#[test]
fn info_reports_cached_xcode_provenance_without_export() {
    let env = Env::new();
    env.write_manifest("[xcode]\nskills = '*'\n");
    let digest = "a".repeat(64);
    fs::write(env.config("skills.lock"), format!("version = 1\nxcode_all_complete = true\n[xcode.apple]\nversion = 'Xcode 27 build TEST'\ndigest = '{digest}'\n")).unwrap();
    let out = env.ok(&["info", "apple"]);
    // A version 1 lock's Xcode entry reads as a member of the xcode repository.
    assert!(out.contains("App version: Xcode 27 build TEST"));
    assert!(out.contains(&format!("Content digest: {digest}")));
    assert!(out.contains("missing SKILL.md"));
    assert!(!env.home.join(".local").exists());
}

#[test]
fn info_reports_local_source_in_agent_directory_as_direct() {
    let env = Env::new();
    write_skill(&env.codex("notes"), "notes", "local");
    env.write_manifest(
        "agents = ['codex']\n[skills]\nnotes = { path = '~/.agents/skills/notes' }\n",
    );
    env.ok(&["sync"]);
    let out = env.ok(&["info", "notes"]);
    assert!(out.contains("~/.agents/skills/notes — local source (direct)"));
    assert!(!out.contains("conflict"));
}

#[test]
fn info_accepts_local_skill_document_symlinks() {
    let env = Env::new();
    let source = env.home.join("notes");
    write_skill(&source, "notes", "local");
    fs::rename(source.join("SKILL.md"), source.join("definition.md")).unwrap();
    std::os::unix::fs::symlink("definition.md", source.join("SKILL.md")).unwrap();
    env.write_manifest("[skills]\nnotes = { path = '~/notes' }\n");
    env.ok(&["sync"]);
    let out = env.ok(&["info", "notes"]);
    assert!(out.contains("Content: ~/notes (available)"));
    assert!(out.contains("~/.claude/skills/notes — installed (owned)"));
    assert!(out.contains("~/.agents/skills/notes — installed (owned)"));
    assert!(!out.contains("broken"));
}

#[test]
fn info_matching_unowned_links_are_not_conflicts() {
    let env = Env::new();
    let source = env.home.join("notes");
    write_skill(&source, "notes", "local");
    fs::create_dir_all(env.claude("notes").parent().unwrap()).unwrap();
    std::os::unix::fs::symlink(&source, env.claude("notes")).unwrap();
    env.write_manifest("[skills]\nnotes = { path = '~/notes' }\n");
    env.ok(&["sync"]);
    let out = env.ok(&["info", "notes"]);
    assert!(out.contains("~/.claude/skills/notes — unmanaged"));
    assert!(!out.contains("conflict"));
    assert!(out.contains("~/.agents/skills/notes — installed (owned)"));
    assert_eq!(fs::read_link(env.claude("notes")).unwrap(), source);
}

#[test]
fn info_rejects_symlinked_skill_documents_in_git_store() {
    let env = Env::new();
    let source = env.repo("info-store", &[("notes", "notes")]);
    env.write_manifest(&format!("[skills]\nnotes = '{source}'\n"));
    env.ok(&["sync"]);
    let content = fs::read_link(env.claude("notes")).unwrap();
    fs::rename(content.join("SKILL.md"), content.join("definition.md")).unwrap();
    std::os::unix::fs::symlink("definition.md", content.join("SKILL.md")).unwrap();
    let out = env.ok(&["info", "notes"]);
    assert!(out.contains("missing SKILL.md"));
    assert!(out.contains("broken (owned)"));
}

#[test]
fn doctor_is_read_only_and_filters_inactive_skills() {
    let env = Env::new();
    env.write_manifest("[skills]\nabsent = { path = '~/absent', profiles = ['work'] }\n");
    assert!(env.ok(&["doctor"]).contains("No problems found"));
    assert!(!env.home.join(".local/share/skl").exists());
    let before = fs::read(env.config("skills.toml")).unwrap();
    let out = env.skl(&["--profile", "work", "doctor"]);
    assert!(!out.status.success());
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("missing-local-skill"));
    assert!(text.contains("missing-link"));
    assert_eq!(before, fs::read(env.config("skills.toml")).unwrap());
    assert!(!env.config("skills.lock").exists());
    assert!(!env.home.join(".local/share/skl").exists());
    assert!(!env.home.join(".agents").exists());
}

#[test]
fn doctor_accepts_matching_user_links_without_claiming_ownership() {
    let env = Env::new();
    write_skill(&env.home.join("local"), "local", "v1");
    env.write_manifest("[skills]\nlocal = { path = '~/local' }\n");
    env.ok(&["sync"]);
    assert!(env.ok(&["doctor"]).contains("No problems found"));
    let state = env.home.join(".local/share/skl/state.toml");
    fs::remove_file(&state).unwrap();
    let out = env.skl(&["doctor"]);
    assert!(out.status.success());
    assert!(String::from_utf8_lossy(&out.stdout).contains("No problems found"));
    assert!(!state.exists());
    assert_eq!(
        fs::read_link(env.codex("local")).unwrap(),
        env.home.join("local")
    );
}

#[test]
fn doctor_reports_repointed_broken_and_stale_owned_links() {
    let env = Env::new();
    write_skill(&env.home.join("local"), "local", "v1");
    env.write_manifest("[skills]\nlocal = { path = '~/local' }\n");
    env.ok(&["sync"]);
    fs::remove_file(env.codex("local")).unwrap();
    std::os::unix::fs::symlink(env.home.join("other"), env.codex("local")).unwrap();
    fs::remove_dir_all(env.home.join("local")).unwrap();
    env.write_manifest("[skills]\n");
    let state = fs::read(env.home.join(".local/share/skl/state.toml")).unwrap();
    let out = env.skl(&["doctor"]);
    assert!(!out.status.success());
    let text = String::from_utf8_lossy(&out.stdout);
    for code in ["stale-state", "broken-link", "link-conflict"] {
        assert!(text.contains(code), "{text}");
    }
    assert_eq!(
        state,
        fs::read(env.home.join(".local/share/skl/state.toml")).unwrap()
    );
}

#[test]
fn doctor_rejects_symlinked_cached_skill_file_without_network() {
    let env = Env::new();
    let url = env.repo("repo", &[("", "example")]);
    env.write_manifest(&format!("[skills]\nexample = '{url}'\n"));
    env.ok(&["sync"]);
    let target = fs::read_link(env.codex("example")).unwrap();
    let skill_file = target.join("SKILL.md");
    fs::rename(&skill_file, target.join("real.md")).unwrap();
    std::os::unix::fs::symlink("real.md", &skill_file).unwrap();
    fs::remove_dir_all(env.remote("repo")).unwrap();
    let out = env.skl(&["doctor"]);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stdout).contains("invalid-cache"));
    assert!(
        fs::symlink_metadata(&skill_file)
            .unwrap()
            .file_type()
            .is_symlink()
    );
}

#[test]
fn doctor_reports_invalid_configuration_without_creating_data() {
    let env = Env::new();
    env.write_manifest("bad = true");
    assert!(env.fails(&["doctor"]).contains("check skills.toml"));
    env.write_manifest("[skills]\n");
    fs::write(env.config("skills.lock"), "version = 999\n").unwrap();
    assert!(env.fails(&["doctor"]).contains("check skills.lock"));
    assert!(!env.home.join(".local/share/skl").exists());
}

#[test]
fn doctor_checks_tools_only_for_enabled_sources() {
    let env = Env::new();
    let other_os = if std::env::consts::OS == "macos" {
        "linux"
    } else {
        "macos"
    };
    env.write_manifest(&format!(
        "[skills]\nabsent = {{ source = 'example/absent', os = ['{other_os}'] }}\n"
    ));
    let empty_path = env.home.join("empty-bin");
    fs::create_dir(&empty_path).unwrap();
    let out = env.skl_with_env(&["doctor"], &[("PATH", empty_path.as_os_str())]);
    assert!(out.status.success());
    env.write_manifest("[skills]\nabsent = 'example/absent'\n");
    let out = env.skl_with_env(&["doctor"], &[("PATH", empty_path.as_os_str())]);
    assert!(!out.status.success());
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("`git` is not executable"));
    assert!(text.contains("`tar` is not executable"));
    assert!(text.contains("not-locked"));
    assert!(!env.home.join(".local/share/skl").exists());
}

#[test]
fn doctor_reports_stale_git_lock_entries() {
    let env = Env::new();
    let url = env.repo("repo", &[("", "example")]);
    env.write_manifest(&format!("[skills]\nexample = '{url}'\n"));
    env.ok(&["sync"]);
    env.write_manifest("[skills]\n");
    fs::remove_file(env.home.join(".local/share/skl/state.toml")).unwrap();
    let out = env.skl(&["doctor"]);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stdout).contains("stale-lock"));
}

#[test]
fn doctor_checks_cached_xcode_selection_without_exporting() {
    let env = Env::new();
    env.write_manifest("[xcode]\nskills = ['example']\nos = ['macos', 'linux']\n");
    let out = env.skl(&["doctor"]);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stdout).contains("invalid-cache: example"));
    env.write_manifest("[xcode]\nskills = '*'\nos = ['macos', 'linux']\n");
    let out = env.skl(&["doctor"]);
    assert!(!out.status.success());
    assert!(
        String::from_utf8_lossy(&out.stdout).contains(
            "source-not-locked: installation (~/.config/skl/skills.lock): source `xcode`"
        )
    );
    env.write_manifest("[xcode]\nskills = '*'\nprofiles = ['inactive']\nos = ['macos', 'linux']\n");
    assert!(env.ok(&["doctor"]).contains("No problems found"));
    env.write_manifest("[skills]\n");
    fs::write(
        env.config("skills.lock"),
        format!(
            "version = 1\n[xcode.example]\nversion = '1'\ndigest = '{}'\n",
            "a".repeat(64)
        ),
    )
    .unwrap();
    let out = env.skl(&["doctor"]);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stdout).contains("stale-lock"));
    assert!(!env.home.join(".local/share/skl").exists());
}

#[test]
fn doctor_allows_retained_git_lock_for_inactive_local_replacement() {
    let env = Env::new();
    let url = env.repo("repo", &[("", "example")]);
    env.write_manifest(&format!("[skills]\nexample = '{url}'\n"));
    env.ok(&["sync"]);
    env.write_manifest("[skills]\nexample = { path = '~/local', profiles = ['inactive'] }\n");
    env.ok(&["sync"]);
    assert!(
        fs::read_to_string(env.config("skills.lock"))
            .unwrap()
            .contains("[skills.example]")
    );
    assert!(env.ok(&["doctor"]).contains("No problems found"));
}

#[test]
fn doctor_accepts_readable_local_skill_symlinks() {
    let env = Env::new();
    write_skill(&env.home.join("local"), "local", "v1");
    fs::rename(
        env.home.join("local/SKILL.md"),
        env.home.join("local/content.md"),
    )
    .unwrap();
    std::os::unix::fs::symlink("content.md", env.home.join("local/SKILL.md")).unwrap();
    env.write_manifest("[skills]\nlocal = { path = '~/local' }\n");
    env.ok(&["sync"]);
    assert!(env.ok(&["doctor"]).contains("No problems found"));
}

#[test]
fn doctor_reports_unreadable_local_skill_file() {
    use std::os::unix::fs::PermissionsExt;
    let env = Env::new();
    write_skill(&env.home.join("local"), "local", "v1");
    env.write_manifest("[skills]\nlocal = { path = '~/local' }\n");
    env.ok(&["sync"]);
    let file = env.home.join("local/SKILL.md");
    fs::set_permissions(&file, fs::Permissions::from_mode(0o000)).unwrap();
    // Root can read mode-000 files; only assert failure when the process cannot.
    if fs::File::open(&file).is_err() {
        let out = env.skl(&["doctor"]);
        assert!(!out.status.success());
        assert!(String::from_utf8_lossy(&out.stdout).contains("unreadable"));
    }
    fs::set_permissions(&file, fs::Permissions::from_mode(0o644)).unwrap();
}

fn json_output(out: &Output) -> serde_json::Value {
    assert!(
        out.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let value: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(value["schema_version"], 2);
    value
}

#[test]
fn json_inspection_reports_missing_and_excluded_skills_without_writes() {
    let env = Env::new();
    env.write_manifest("agents = ['codex']\n[skills]\nz = { path = '~/z', profiles = ['work'] }\na = { path = '~/a' }\n");
    let before = fs::read(env.config("skills.toml")).unwrap();
    let out = env.skl(&["list", "--json"]);
    assert!(!out.status.success());
    let value = json_output(&out);
    assert_eq!(value["status"], "unhealthy");
    assert_eq!(value["data"]["skills"][0]["name"], "a");
    assert_eq!(value["data"]["skills"][1]["enabled"], false);
    assert_eq!(value["data"]["skills"][0]["links"][0]["status"], "missing");
    assert_eq!(
        value["data"]["skills"][0]["content_path"],
        env.home.join("a").to_str().unwrap()
    );
    assert_eq!(value["diagnostics"].as_array().unwrap().len(), 1);
    assert_eq!(value["diagnostics"][0]["code"], "not-linked");
    assert_eq!(env.skl(&["list", "--json"]).stdout, out.stdout);
    let out = env.skl(&["info", "a", "--json"]);
    assert!(out.status.success());
    assert_eq!(json_output(&out)["data"]["content_available"], false);
    let out = env.skl(&["doctor", "--json"]);
    assert!(!out.status.success());
    let diagnostics = json_output(&out)["diagnostics"].as_array().unwrap().clone();
    assert!(
        diagnostics
            .iter()
            .any(|d| d["code"] == "missing-local-skill")
    );
    assert!(diagnostics.iter().any(|d| d["code"] == "missing-link"));
    assert!(
        diagnostics
            .iter()
            .all(|d| d["severity"] == "error" && d["remedy"].is_string())
    );
    assert_eq!(fs::read(env.config("skills.toml")).unwrap(), before);
    assert!(!env.config("skills.lock").exists());
    assert!(!env.home.join(".local").exists());
}

#[test]
fn json_runtime_errors_are_single_documents() {
    let env = Env::new();
    for file in ["skills.toml", "skills.lock"] {
        env.write_manifest("[skills]\na = { path = '~/a' }\n");
        fs::write(env.config(file), "invalid = [").unwrap();
        for args in [
            vec!["list", "--json"],
            vec!["info", "a", "--json"],
            vec!["doctor", "--json"],
        ] {
            let out = env.skl(&args);
            assert!(!out.status.success());
            let value = json_output(&out);
            assert_eq!(value["status"], "error");
            assert!(value["data"].is_null());
            assert_eq!(value["diagnostics"][0]["code"], "runtime-error");
            assert!(
                value["diagnostics"][0]["message"]
                    .as_str()
                    .unwrap()
                    .contains(file)
            );
        }
    }
    assert!(!env.home.join(".local").exists());
    let out = env.skl_with_env(&["list", "--json"], &[("HOME", std::ffi::OsStr::new(""))]);
    assert!(!out.status.success());
    assert_eq!(json_output(&out)["status"], "error");
}

#[test]
fn json_includes_git_provenance_owned_links_and_unmanaged_entries() {
    let env = Env::new();
    let url = env.repo("notes", &[("", "notes")]);
    env.ok(&["add", &url]);
    fs::write(env.codex("loose"), "user file").unwrap();
    std::os::unix::fs::symlink("loose", env.codex("alias")).unwrap();
    let state = env.home.join(".local/share/skl/state.toml");
    let before = [
        fs::read(&state).unwrap(),
        fs::read(env.config("skills.lock")).unwrap(),
    ];
    let out = env.skl(&["list", "--json"]);
    assert!(out.status.success());
    let value = json_output(&out);
    let skill = &value["data"]["skills"][0];
    assert_eq!(skill["source"], url);
    assert_eq!(
        skill["locked"]["commit"],
        git(&env.remote("notes"), &["rev-parse", "HEAD"])
    );
    assert_eq!(skill["links"][0]["owned"], true);
    let unmanaged = value["data"]["unmanaged"].as_array().unwrap();
    assert_eq!(unmanaged.len(), 2);
    assert_eq!(unmanaged[0]["name"], "alias");
    assert_eq!(unmanaged[0]["target"], env.codex("loose").to_str().unwrap());
    let out = env.skl(&["info", "alias", "--json"]);
    assert_eq!(
        json_output(&out)["data"]["links"][0]["target"],
        env.codex("loose").to_str().unwrap()
    );
    let out = env.skl(&["doctor", "--json"]);
    assert!(out.status.success());
    assert_eq!(json_output(&out)["status"], "ok");
    assert_eq!(
        before,
        [
            fs::read(state).unwrap(),
            fs::read(env.config("skills.lock")).unwrap()
        ]
    );
}

#[test]
fn json_reports_cached_and_undiscovered_xcode_without_exporting() {
    let env = Env::new();
    env.write_manifest("[xcode]\nskills = '*'\nos = ['macos', 'linux']\n");
    let out = env.skl(&["list", "--json"]);
    assert!(!out.status.success());
    assert_eq!(
        json_output(&out)["diagnostics"][0]["code"],
        "source-not-locked"
    );
    let digest = "a".repeat(64);
    fs::write(env.config("skills.lock"), format!("version = 1\nxcode_all_complete = true\n[xcode.apple]\nversion = 'Xcode TEST'\ndigest = '{digest}'\n")).unwrap();
    let out = env.skl(&["info", "apple", "--json"]);
    assert!(out.status.success());
    let value = json_output(&out);
    assert_eq!(value["data"]["origin"], "app");
    assert_eq!(value["data"]["source"], "xcode");
    assert_eq!(value["data"]["command"]["source"], "xcode");
    assert_eq!(value["data"]["command"]["snapshot"]["digest"], digest);
    let value = json_output(&env.skl(&["list", "--json"]));
    assert_eq!(value["data"]["skills"][0], json_output(&out)["data"]);
    assert!(!env.home.join(".local").exists());
}

#[cfg(target_os = "linux")]
#[test]
fn json_non_utf8_unmanaged_paths_return_an_error_document() {
    use std::os::unix::ffi::OsStringExt;
    let env = Env::new();
    env.write_manifest("agents = ['codex']\n");
    fs::create_dir_all(env.codex("")).unwrap();
    let path = env
        .codex("")
        .join(std::ffi::OsString::from_vec(b"invalid-\xff".to_vec()));
    fs::write(path, "user file").unwrap();
    let out = env.skl(&["list", "--json"]);
    assert!(!out.status.success());
    assert_eq!(json_output(&out)["status"], "error");
    assert!(!env.home.join(".local").exists());
}

#[test]
fn discover_lists_skills_descriptions_and_commit_without_installing() {
    let env = Env::new();
    let url = env.repo("browse", &[("z", "shared"), ("a", "shared"), ("", "root")]);
    fs::write(
        env.remote("browse").join("a/SKILL.md"),
        "---\nname: shared\ndescription: >-\n  First line\n  second line\n---\nBody\n",
    )
    .unwrap();
    fs::write(
        env.remote("browse").join("z/SKILL.md"),
        "---\nname: shared\ndescription: 'It''s helpful # really' # user's comment\n---  \n",
    )
    .unwrap();
    let sha = commit(&env.remote("browse"), "descriptions");
    let out = env.ok(&["discover", &url]);
    assert!(out.contains(&format!("Commit: {sha}")));
    assert!(out.contains("root\t.\ttest"));
    assert!(out.contains("shared\ta\tFirst line second line"));
    assert!(out.contains("shared\tz\tIt's helpful # really"));
    assert!(out.find("shared\ta").unwrap() < out.find("shared\tz").unwrap());
    assert_eq!(fs::read_dir(&env.home).unwrap().count(), 0);
}

#[test]
fn discover_honors_ref_and_does_not_read_or_modify_installed_state() {
    let env = Env::new();
    let url = env.repo("browse", &[("old", "old")]);
    let sha = git(&env.remote("browse"), &["rev-parse", "HEAD"]);
    git(&env.remote("browse"), &["tag", "before"]);
    fs::remove_dir_all(env.remote("browse").join("old")).unwrap();
    write_skill(&env.remote("browse").join("new"), "new", "new");
    commit(&env.remote("browse"), "replace");
    env.write_manifest("invalid manifest [[[");
    let data = env.home.join(".local/share/skl");
    fs::create_dir_all(&data).unwrap();
    fs::write(data.join("state.toml"), "invalid state").unwrap();
    fs::write(env.config("skills.lock"), "invalid lock").unwrap();
    let out = env.ok(&["discover", &url, "--ref", "before"]);
    assert!(out.contains(&format!("Commit: {sha}")));
    assert!(out.contains("old\told\ttest"));
    assert!(!out.contains("new\tnew"));
    assert!(
        env.ok(&["discover", &url, "--ref", &sha])
            .contains("old\told")
    );
    assert!(env.ok(&["discover", &url]).contains("new\tnew"));
    assert_eq!(
        fs::read_to_string(env.config("skills.toml")).unwrap(),
        "invalid manifest [[["
    );
    assert_eq!(
        fs::read_to_string(env.config("skills.lock")).unwrap(),
        "invalid lock"
    );
    assert_eq!(
        fs::read_to_string(data.join("state.toml")).unwrap(),
        "invalid state"
    );
    assert_eq!(fs::read_dir(data).unwrap().count(), 1);
    assert!(
        env.fails(&["discover", &url, "--ref", "missing"])
            .contains("no branch or tag")
    );
}

#[test]
fn discover_keeps_unique_hidden_skills_and_excludes_symlink_documents() {
    let env = Env::new();
    let url = env.repo(
        "browse",
        &[("visible", "visible"), (".agents/hidden", "hidden")],
    );
    let symlink = env.remote("browse").join("symlink");
    fs::create_dir_all(&symlink).unwrap();
    std::os::unix::fs::symlink("../visible/SKILL.md", symlink.join("SKILL.md")).unwrap();
    commit(&env.remote("browse"), "symlink");
    let out = env.ok(&["discover", &url]);
    assert!(out.contains("visible\tvisible"));
    assert!(out.contains("hidden\t.agents/hidden"));
    assert!(!out.contains("symlink"));
    fs::remove_dir_all(env.remote("browse").join("visible")).unwrap();
    commit(&env.remote("browse"), "hidden only");
    assert!(
        env.ok(&["discover", &url])
            .contains("hidden\t.agents/hidden")
    );
    fs::remove_dir_all(env.remote("browse").join(".agents")).unwrap();
    commit(&env.remote("browse"), "no regular skills");
    assert!(env.ok(&["discover", &url]).contains("No skills found"));
    assert_eq!(fs::read_dir(&env.home).unwrap().count(), 0);
}

#[test]
fn discover_handles_missing_and_quoted_descriptions_without_changing_names() {
    let env = Env::new();
    let url = env.repo(
        "browse",
        &[
            ("plain", "plain"),
            ("quoted", "quoted"),
            ("missing", "missing"),
        ],
    );
    fs::write(
        env.remote("browse").join("plain/SKILL.md"),
        "---\nname: plain\ndescription: useful # comment\n---\n",
    )
    .unwrap();
    fs::write(
        env.remote("browse").join("quoted/SKILL.md"),
        "---\nname: quoted\ndescription: \"A \\\"quote\\\" and tab\\ttext\"\n---\n",
    )
    .unwrap();
    fs::write(
        env.remote("browse").join("missing/SKILL.md"),
        "No frontmatter\ndescription: not metadata\n",
    )
    .unwrap();
    commit(&env.remote("browse"), "description formats");
    let out = env.ok(&["discover", &url]);
    assert!(out.contains("plain\tplain\tuseful\n"));
    assert!(out.contains("quoted\tquoted\tA \"quote\" and tab text\n"));
    assert!(out.contains("missing\tmissing\t\n"));
}

#[test]
fn add_skill_options_and_existing_entry_are_preserved() {
    let env = Env::new();
    let url = env.repo("options", &[(".", "example")]);
    env.ok(&[
        "add",
        &url,
        "--agent",
        "codex",
        "--agent",
        "codex",
        "--agent",
        "~/custom/skills",
        "--os",
        "linux",
        "--os",
        "macos",
        "--only-profile",
        "work",
        "--no-sync",
    ]);
    let text = fs::read_to_string(env.config("skills.toml")).unwrap();
    let doc: toml::Value = toml::from_str(&text).unwrap();
    let skill = &doc["skills"]["example"];
    assert_eq!(skill["source"].as_str(), Some(url.as_str()));
    assert_eq!(skill["agents"].as_array().unwrap().len(), 2);
    assert_eq!(skill["os"].as_array().unwrap().len(), 2);
    assert_eq!(skill["profiles"][0].as_str(), Some("work"));
    env.ok(&["add", &url, "--agent", "claude-code", "--no-sync"]);
    assert_eq!(fs::read_to_string(env.config("skills.toml")).unwrap(), text);
    assert!(!env.config("skills.lock").exists());
}

#[test]
fn configure_batch_preserves_source_fields_comments_and_symlink() {
    let env = Env::new();
    let original = "# settings\n[skills]\nshort = 'owner/repo' # keep shorthand comment\ninline = { source = 'owner/repo', ref = 'v1', subdir = 'skills/x', os = ['macos'] } # inline comment\n\n[skills.local]\n# local directory\npath = '~/missing'\nagents = ['codex'] # agent comment\n";
    env.write_manifest(original);
    let target = env.home.join("dotfiles.toml");
    fs::rename(env.config("skills.toml"), &target).unwrap();
    std::os::unix::fs::symlink(&target, env.config("skills.toml")).unwrap();
    env.ok(&[
        "configure",
        "short",
        "inline",
        "local",
        "--agent",
        "cursor",
        "--only-profile",
        "work",
        "--no-sync",
    ]);
    let text = fs::read_to_string(&target).unwrap();
    for comment in [
        "# settings",
        "# keep shorthand comment",
        "# inline comment",
        "# local directory",
        "# agent comment",
    ] {
        assert!(text.contains(comment), "missing {comment}: {text}");
    }
    let doc: toml::Value = toml::from_str(&text).unwrap();
    assert_eq!(doc["skills"]["inline"]["ref"].as_str(), Some("v1"));
    assert_eq!(doc["skills"]["inline"]["subdir"].as_str(), Some("skills/x"));
    assert_eq!(doc["skills"]["inline"]["os"][0].as_str(), Some("macos"));
    assert_eq!(doc["skills"]["local"]["path"].as_str(), Some("~/missing"));
    for name in ["short", "inline", "local"] {
        assert_eq!(doc["skills"][name]["agents"][0].as_str(), Some("cursor"));
        assert_eq!(doc["skills"][name]["profiles"][0].as_str(), Some("work"));
    }
    assert!(
        fs::symlink_metadata(env.config("skills.toml"))
            .unwrap()
            .is_symlink()
    );
    assert!(!env.home.join(".local/share/skl/repos").exists());
}

#[test]
fn configure_resets_remove_overrides_without_expanding_shorthand() {
    let env = Env::new();
    env.write_manifest("[skills]\nshort = 'owner/repo' # unchanged\ninline = { source = 'owner/repo', agents = ['codex'], os = ['linux'], profiles = ['work'] }\n[skills.normal]\npath = '~/missing'\nagents = ['codex']\nos = ['linux']\nprofiles = ['work']\n");
    env.ok(&[
        "configure",
        "short",
        "inline",
        "normal",
        "--default-agents",
        "--all-os",
        "--all-profiles",
        "--no-sync",
    ]);
    let text = fs::read_to_string(env.config("skills.toml")).unwrap();
    assert!(text.contains("short = 'owner/repo' # unchanged"));
    let doc: toml::Value = toml::from_str(&text).unwrap();
    for name in ["inline", "normal"] {
        let skill = doc["skills"][name].as_table().unwrap();
        assert!(!skill.contains_key("agents"));
        assert!(!skill.contains_key("os"));
        assert!(!skill.contains_key("profiles"));
    }
}

#[test]
fn configure_invalid_edits_are_atomic() {
    let env = Env::new();
    let original = "[skills]\nx = 'owner/repo'\n";
    env.write_manifest(original);
    for args in [
        vec!["configure", "x", "missing", "--agent", "codex", "--no-sync"],
        vec!["configure", "x", "--agent", "unknown-harness", "--no-sync"],
        vec!["configure", "x", "--agent", "relative/path", "--no-sync"],
        vec![
            "configure",
            "x",
            "--only-profile",
            "bad/profile",
            "--no-sync",
        ],
        vec!["configure", "x", "--os", "windows", "--no-sync"],
        vec![
            "configure",
            "x",
            "--agent",
            "codex",
            "--default-agents",
            "--no-sync",
        ],
        vec!["configure", "x", "--os", "linux", "--all-os", "--no-sync"],
        vec![
            "configure",
            "x",
            "--only-profile",
            "work",
            "--all-profiles",
            "--no-sync",
        ],
    ] {
        env.fails(&args);
        assert_eq!(
            fs::read_to_string(env.config("skills.toml")).unwrap(),
            original
        );
    }
    let invalid = "[skills]\nx = 'owner/repo'\ny = {path = '~/missing', bogus = true}\n";
    env.write_manifest(invalid);
    env.fails(&["configure", "x", "--agent", "codex", "--no-sync"]);
    assert_eq!(
        fs::read_to_string(env.config("skills.toml")).unwrap(),
        invalid
    );
}

#[test]
fn configure_sync_uses_global_profile_and_reconciles_links() {
    let env = Env::new();
    write_skill(&env.home.join("local"), "example", "v1");
    env.ok(&[
        "add",
        "./local",
        "--agent",
        "codex",
        "--only-profile",
        "work",
        "--profile",
        "work",
    ]);
    assert!(env.codex("example").is_symlink());
    assert!(!env.claude("example").exists());
    env.ok(&[
        "configure",
        "example",
        "--default-agents",
        "--profile",
        "work",
    ]);
    assert!(env.claude("example").is_symlink());
    env.ok(&[
        "configure",
        "example",
        "--only-profile",
        "personal",
        "--profile",
        "work",
    ]);
    assert!(!env.codex("example").exists());
    assert!(!env.claude("example").exists());
}

#[test]
fn add_invalid_options_or_manifest_leave_manifest_unchanged() {
    let env = Env::new();
    write_skill(&env.home.join("local"), "example", "v1");
    let original = "[skills]\nx = 'owner/repo'\n";
    env.write_manifest(original);
    env.fails(&["add", "./local", "--agent", "unknown-harness", "--no-sync"]);
    env.fails(&[
        "add",
        "./local",
        "--only-profile",
        "bad/profile",
        "--no-sync",
    ]);
    assert_eq!(
        fs::read_to_string(env.config("skills.toml")).unwrap(),
        original
    );
    let invalid = "[skills]\nx = {source = 'owner/repo', invalid = true}\n";
    env.write_manifest(invalid);
    env.fails(&["add", "./local", "--no-sync"]);
    assert_eq!(
        fs::read_to_string(env.config("skills.toml")).unwrap(),
        invalid
    );
}

#[test]
fn add_local_collection_all_preserves_unique_hidden_skills_and_relative_paths() {
    let env = Env::new();
    let root = env.config("collection");
    write_skill(&root.join("visible"), "visible", "canonical");
    write_skill(&root.join(".agents/visible"), "visible", "mirror");
    write_skill(&root.join(".curated/curated"), "curated", "curated");
    write_skill(&root.join(".experimental/next"), "next", "next");
    write_skill(&root.join(".git/ignored"), "ignored", "ignored");
    let outside = env.home.join("outside");
    write_skill(&outside, "outside", "outside");
    std::os::unix::fs::symlink(&outside, root.join("linked-directory")).unwrap();
    fs::create_dir_all(root.join("linked-document")).unwrap();
    std::os::unix::fs::symlink(
        outside.join("SKILL.md"),
        root.join("linked-document/SKILL.md"),
    )
    .unwrap();
    env.ok(&["add", ".config/skl/collection", "--all"]);
    let manifest = fs::read_to_string(env.config("skills.toml")).unwrap();
    assert!(manifest.contains("collection/visible"));
    assert!(!manifest.contains("~/"));
    assert!(!manifest.contains("linked-"));
    assert!(!manifest.contains("ignored"));
    assert_eq!(
        fs::read_link(env.claude("visible")).unwrap(),
        root.join("visible")
    );
    assert!(body(&env.claude("visible")).contains("canonical"));
    assert!(env.claude("curated").exists());
    assert!(env.claude("next").exists());
    assert!(!env.claude("outside").exists());
    assert!(
        fs::read_to_string(env.config("skills.lock"))
            .unwrap()
            .contains("version = 3")
    );
}

#[test]
fn add_local_collection_selection_and_failures_are_atomic() {
    let env = Env::new();
    let root = env.home.join("collection");
    write_skill(&root.join("first"), "one", "one");
    write_skill(&root.join("second"), "two", "two");
    assert!(env.fails(&["add", "collection"]).contains("--all"));
    assert!(!env.config("skills.toml").exists());
    env.fails(&["add", "collection", "--skill", "one", "--skill", "missing"]);
    assert!(!env.config("skills.toml").exists());
    env.ok(&[
        "add",
        "collection",
        "--skill",
        "one",
        "--skill",
        "second",
        "--agent",
        "codex",
    ]);
    assert!(env.codex("one").exists());
    assert!(env.codex("second").exists());
    assert!(!env.claude("one").exists());
    let manifest = fs::read_to_string(env.config("skills.toml")).unwrap();
    env.fails(&["add", "collection", "--all", "--ref", "main"]);
    assert_eq!(
        manifest,
        fs::read_to_string(env.config("skills.toml")).unwrap()
    );
}

#[test]
fn local_collection_ambiguity_is_sorted_and_never_writes_manifest() {
    let env = Env::new();
    let root = env.home.join("collection");
    write_skill(&root.join("z"), "same", "z");
    write_skill(&root.join("a"), "same", "a");
    for args in [
        vec!["add", "collection", "--all", "--no-sync"],
        vec!["add", "collection", "--skill", "same"],
    ] {
        assert!(env.fails(&args).contains("(a, z)"));
        assert!(!env.config("skills.toml").exists());
    }
    env.ok(&["add", "collection/a", "--skill", "renamed"]);
    assert!(env.claude("renamed").exists());
}

#[test]
fn git_hidden_collections_coexist_with_visible_skills_and_reject_ambiguous_adds() {
    let env = Env::new();
    let url = env.repo(
        "collections",
        &[
            ("visible", "visible"),
            (".agents/visible", "visible"),
            (".curated/one", "one"),
            (".experimental/two", "two"),
        ],
    );
    env.ok(&["add", &url, "--all"]);
    assert!(env.claude("visible").exists());
    assert!(env.claude("one").exists());
    assert!(env.claude("two").exists());
    let ambiguous = env.repo(
        "ambiguous",
        &[
            (".curated/a", "duplicate"),
            (".experimental/b", "duplicate"),
        ],
    );
    let manifest = fs::read_to_string(env.config("skills.toml")).unwrap();
    assert!(
        env.fails(&["add", &ambiguous, "--all", "--no-sync"])
            .contains("(.curated/a, .experimental/b)")
    );
    assert_eq!(
        manifest,
        fs::read_to_string(env.config("skills.toml")).unwrap()
    );
    let discovery = env.ok(&["discover", &ambiguous]);
    assert!(discovery.contains(".curated/a"));
    assert!(discovery.contains(".experimental/b"));
}

#[test]
fn local_collection_empty_and_root_skill_behavior() {
    let env = Env::new();
    fs::create_dir_all(env.home.join("empty")).unwrap();
    assert!(
        env.fails(&["add", "empty", "--all"])
            .contains("no regular SKILL.md")
    );
    assert!(!env.config("skills.toml").exists());
    let root = env.home.join("collection");
    write_skill(&root, "parent", "parent");
    write_skill(&root.join("nested"), "child", "child");
    env.ok(&["add", "collection", "--skill", "renamed"]);
    assert!(env.claude("renamed").exists());
    assert!(!env.claude("child").exists());
    env.ok(&["add", "collection", "--all"]);
    assert!(env.claude("parent").exists());
    assert!(env.claude("child").exists());
    fs::write(root.join("nested/SKILL.md"), "live edit").unwrap();
    assert_eq!(body(&env.claude("child")), "live edit");
}

#[test]
fn source_inputs_tree_urls_and_shorthand_normalize() {
    for hosted in [
        "https://github.com/acme/catalog",
        "https://gitlab.example/acme/team/catalog",
    ] {
        let env = Env::new();
        let url = env.repo("catalog", &[("skills/one", "one"), ("skills/two", "two")]);
        // Use an explicit per-test config, never the real user's global config.
        fs::write(
            env.home.join(".gitconfig"),
            format!("[url \"{url}\"]\n insteadOf = {hosted}\n"),
        )
        .unwrap();
        let marker = if hosted.contains("github.com") {
            "tree"
        } else {
            "-/tree"
        };
        let tree = format!("{hosted}/{marker}/main/skills/one");
        let discovered = env.ok(&["discover", &tree]);
        assert!(discovered.contains("one\tskills/one"));
        assert!(!discovered.contains("two\tskills/two"));
        env.ok(&["add", &tree]);
        assert!(env.claude("one").is_symlink());
        let manifest: toml::Table = fs::read_to_string(env.config("skills.toml"))
            .unwrap()
            .parse()
            .unwrap();
        assert_eq!(manifest["skills"]["one"]["source"].as_str(), Some(hosted));
        assert_eq!(manifest["skills"]["one"]["ref"].as_str(), Some("main"));
        assert_eq!(
            manifest["skills"]["one"]["subdir"].as_str(),
            Some("skills/one")
        );
        env.ok(&["sync", "--locked"]);
    }
    let env = Env::new();
    let url = env.repo("catalog", &[("one", "one"), ("two", "two")]);
    fs::write(
        env.home.join(".gitconfig"),
        format!("[url \"{url}\"]\n insteadOf = https://github.com/acme/catalog.git\n"),
    )
    .unwrap();
    env.ok(&["add", "acme/catalog@two"]);
    assert!(env.claude("two").is_symlink());
    assert!(!env.claude("one").exists());
    assert!(
        env.fails(&["add", "acme/catalog@one", "--all"])
            .contains("conflicts")
    );
}

#[test]
fn explicit_subdir_selects_hidden_duplicate_and_root() {
    let env = Env::new();
    let url = env.repo(
        "catalog",
        &[("", "root"), ("visible", "same"), (".hidden", "same")],
    );
    env.ok(&["add", &url, "--subdir", ".hidden"]);
    assert!(
        fs::read_to_string(env.config("skills.lock"))
            .unwrap()
            .contains(".hidden")
    );
    env.ok(&["add", &url, "--subdir", "."]);
    assert!(env.claude("root").is_symlink());
    env.ok(&["sync", "--locked"]);
    let before = fs::read(env.config("skills.toml")).unwrap();
    assert!(
        env.fails(&["add", &url, "--subdir", "../escape"])
            .contains("inside")
    );
    env.fails(&["add", &url, "--subdir", "missing"]);
    assert_eq!(before, fs::read(env.config("skills.toml")).unwrap());
}

#[test]
fn tree_url_explicit_slash_ref_and_conflicts() {
    let env = Env::new();
    let url = env.repo("catalog", &[("skills/one", "one")]);
    git(&env.remote("catalog"), &["branch", "feature/topic"]);
    fs::write(
        env.home.join(".gitconfig"),
        format!("[url \"{url}\"]\n insteadOf = https://github.com/acme/catalog\n"),
    )
    .unwrap();
    let tree = "https://github.com/acme/catalog/tree/feature/topic/skills/one";
    assert!(env.fails(&["add", tree]).contains("specify --ref"));
    env.ok(&["add", tree, "--ref", "feature/topic"]);
    assert!(
        fs::read_to_string(env.config("skills.toml"))
            .unwrap()
            .contains("feature/topic")
    );
    assert!(
        env.fails(&["add", tree, "--ref", "main"])
            .contains("does not match")
    );
    assert!(
        env.fails(&[
            "add",
            tree,
            "--ref",
            "feature/topic",
            "--subdir",
            "elsewhere"
        ])
        .contains("conflicts")
    );
}

#[test]
fn project_scope_is_explicit_and_isolated_from_global_and_other_projects() {
    let e = Env::new();
    let url = e.repo("scopes", &[(".", "shared")]);
    e.ok(&["add", &url]);
    let global_target = fs::read_link(e.codex("shared")).unwrap();
    let global_lock = fs::read(e.config("skills.lock")).unwrap();
    let a = e.home.join("project-a");
    let b = e.home.join("project-b");
    for root in [&a, &b] {
        fs::create_dir(root).unwrap();
        e.ok(&["--project", root.to_str().unwrap(), "add", &url]);
        assert!(root.join("skills.toml").is_file());
        assert!(root.join("skills.lock").is_file());
        assert!(root.join(".claude/skills/shared/SKILL.md").is_file());
    }
    let a_target = fs::read_link(a.join(".agents/skills/shared")).unwrap();
    let b_target = fs::read_link(b.join(".agents/skills/shared")).unwrap();
    assert_ne!(a_target, b_target);
    assert_ne!(a_target, global_target);
    e.ok(&["remove", "shared", "--project", a.to_str().unwrap()]);
    assert!(!a.join(".agents/skills/shared").exists());
    assert!(b_target.join("SKILL.md").is_file());
    assert!(global_target.join("SKILL.md").is_file());
    assert_eq!(fs::read(e.config("skills.lock")).unwrap(), global_lock);
    // Even with a project manifest in the working directory, scope is explicit.
    fs::write(e.home.join("skills.toml"), "invalid toml").unwrap();
    e.ok(&["sync", "--locked"]);
}

#[test]
fn project_local_manifest_and_lock_are_portable_to_a_new_checkout() {
    let e = Env::new();
    let a = e.home.join("original");
    let b = e.home.join("checkout");
    write_skill(&a.join("skills/local"), "local", "local contents");
    e.ok(&[
        "--project",
        a.to_str().unwrap(),
        "add",
        "original/skills/local",
    ]);
    let manifest = fs::read_to_string(a.join("skills.toml")).unwrap();
    assert!(manifest.contains("path = \"skills/local\""), "{manifest}");
    let url = e.repo("portable", &[(".", "remote")]);
    e.ok(&["--project", a.to_str().unwrap(), "add", &url]);
    write_skill(&b.join("skills/local"), "local", "checkout contents");
    for name in ["skills.toml", "skills.lock"] {
        fs::copy(a.join(name), b.join(name)).unwrap();
    }
    let lock_before = fs::read(b.join("skills.lock")).unwrap();
    e.ok(&["--project", b.to_str().unwrap(), "sync", "--locked"]);
    assert_eq!(fs::read(b.join("skills.lock")).unwrap(), lock_before);
    assert_eq!(
        fs::canonicalize(b.join(".agents/skills/local")).unwrap(),
        fs::canonicalize(b.join("skills/local")).unwrap()
    );
    assert!(b.join(".agents/skills/remote/SKILL.md").is_file());
    assert!(a.join(".agents/skills/remote/SKILL.md").is_file());
    assert!(!e.config("skills.toml").exists());
    assert!(!e.codex("remote").exists());
}

#[test]
fn project_dry_run_and_locked_failure_do_not_publish_state_or_lock() {
    let e = Env::new();
    let project = e.home.join("project");
    write_skill(&project.join("local"), "local", "v1");
    fs::write(
        project.join("skills.toml"),
        "[skills]\nlocal = { path = 'local' }\n",
    )
    .unwrap();
    let unused_config = e.home.join("unused-config");
    let unused_data = e.home.join("unused-data");
    let out = e.skl_with_env(
        &["--project", project.to_str().unwrap(), "sync", "--dry-run"],
        &[
            ("SKL_CONFIG_DIR", unused_config.as_os_str()),
            ("SKL_DATA_DIR", unused_data.as_os_str()),
        ],
    );
    assert!(out.status.success());
    assert!(!unused_config.exists());
    assert!(!unused_data.exists());
    assert!(!project.join("skills.lock").exists());
    assert!(!project.join(".agents").exists());
    let url = e.repo("locked-project", &[(".", "remote")]);
    fs::write(
        project.join("skills.toml"),
        format!("[skills]\nremote = '{url}'\n"),
    )
    .unwrap();
    e.fails(&["--project", project.to_str().unwrap(), "sync", "--locked"]);
    assert!(!project.join("skills.lock").exists());
    assert!(!project.join(".agents").exists());
}

#[test]
fn project_move_preserves_ownership_and_requires_force_for_old_links() {
    let e = Env::new();
    let a = e.home.join("original");
    let b = e.home.join("moved");
    fs::create_dir(&a).unwrap();
    let url = e.repo("moved", &[(".", "remote")]);
    e.ok(&["--project", a.to_str().unwrap(), "add", &url]);
    let old_target = fs::read_link(a.join(".agents/skills/remote")).unwrap();
    fs::rename(&a, &b).unwrap();
    // A copied/moved link is not owned by the new root's state.
    let output = e.skl(&["--project", b.to_str().unwrap(), "sync", "--locked"]);
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(combined.contains("skl didn't create it"), "{combined}");
    assert_eq!(
        fs::read_link(b.join(".agents/skills/remote")).unwrap(),
        old_target
    );
    e.ok(&[
        "--project",
        b.to_str().unwrap(),
        "sync",
        "--locked",
        "--force",
    ]);
    assert_ne!(
        fs::read_link(b.join(".agents/skills/remote")).unwrap(),
        old_target
    );
    assert!(old_target.join("SKILL.md").is_file());
}

#[test]
fn project_agent_paths_support_project_only_and_custom_relative_harnesses() {
    let e = Env::new();
    let project = e.home.join("project");
    write_skill(&project.join("local"), "local", "v1");
    fs::write(project.join("skills.toml"), "agents = ['eve', 'promptscript', 'cursor', './custom/skills']\n[skills]\nlocal = { path = 'local' }\n").unwrap();
    e.ok(&["--project", project.to_str().unwrap(), "sync"]);
    for dir in ["agent/skills", ".agents/skills", "custom/skills"] {
        assert!(project.join(dir).join("local/SKILL.md").is_file());
    }
    let listing = e.ok(&["--project", project.to_str().unwrap(), "agents", "list"]);
    assert!(listing.contains("promptscript"));
    assert!(listing.contains("~/project/.agents/skills"));
    e.fails(&["agents", "set", "eve"]);
}

#[test]
fn project_collection_import_keeps_every_source_portable() {
    let e = Env::new();
    let project = e.home.join("project");
    write_skill(&project.join("sources/one"), "one", "one");
    write_skill(&project.join("sources/.curated/two"), "two", "two");
    e.ok(&[
        "--project",
        project.to_str().unwrap(),
        "add",
        "project/sources",
        "--all",
    ]);
    let manifest: toml::Table = fs::read_to_string(project.join("skills.toml"))
        .unwrap()
        .parse()
        .unwrap();
    assert_eq!(
        manifest["skills"]["one"]["path"].as_str(),
        Some("sources/one")
    );
    assert_eq!(
        manifest["skills"]["two"]["path"].as_str(),
        Some("sources/.curated/two")
    );
    e.ok(&["--project", project.to_str().unwrap(), "sync", "--locked"]);
    assert!(project.join(".agents/skills/one/SKILL.md").is_file());
    assert!(project.join(".agents/skills/two/SKILL.md").is_file());
    assert!(!e.config("skills.toml").exists());
}

/// One-request loopback catalog. Never contacts skills.sh, even on failing tests.
fn search_server(status: &str, body: String) -> (String, std::thread::JoinHandle<String>) {
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::time::{Duration, Instant};
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let url = format!("http://{}/api/search", listener.local_addr().unwrap());
    let status = status.to_owned();
    let handle = std::thread::spawn(move || {
        let start = Instant::now();
        let mut stream = loop {
            match listener.accept() {
                Ok((stream, _)) => break stream,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    assert!(
                        start.elapsed() < Duration::from_secs(10),
                        "mock search was never requested"
                    );
                    std::thread::sleep(Duration::from_millis(5));
                }
                Err(error) => panic!("mock accept: {error}"),
            }
        };
        stream.set_nonblocking(false).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        stream
            .set_write_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut request = Vec::new();
        while !request.ends_with(b"\r\n\r\n") {
            let mut byte = [0];
            stream.read_exact(&mut byte).unwrap();
            request.push(byte[0]);
            assert!(request.len() < 8192);
        }
        let response = format!(
            "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        // The client may close early on an oversized body.
        let _ = stream.write_all(response.as_bytes());
        String::from_utf8(request).unwrap()
    });
    (url, handle)
}

fn search_command(env: &Env, endpoint: &str, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_skl"))
        .args(args)
        .current_dir(&env.home)
        .env_remove("HOME")
        .env_remove("HTTP_PROXY")
        .env_remove("HTTPS_PROXY")
        .env_remove("ALL_PROXY")
        .env_remove("http_proxy")
        .env_remove("https_proxy")
        .env_remove("all_proxy")
        .env("SKL_SEARCH_URL", endpoint)
        .output()
        .unwrap()
}

#[test]
fn search_encodes_queries_filters_limits_and_suggests_safe_commands_without_home() {
    let env = Env::new();
    let (url, server) = search_server(
        "200 OK",
        serde_json::json!({"skills": [
            {"name": "small", "source": "example/skills", "installs": 3},
            {"name": "-popular", "source": "example/repo", "installs": 20},
            {"name": "other", "source": "another/repo", "installs": 99}
        ]})
        .to_string(),
    );
    let out = search_command(
        &env,
        &url,
        &[
            "search", "rust", "a&b", "--owner", "Example", "--limit", "1",
        ],
    );
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(text.contains("example/repo/-popular (20 installs)"));
    assert!(text.contains("skl add example/repo --skill=-popular"));
    assert!(!text.contains("small"));
    assert!(!text.contains("another"));
    let request = server.join().unwrap();
    assert!(request.contains("q=rust%20a%26b"), "{request}");
    assert!(request.contains("limit=1"));
    assert!(request.contains("owner=example"));
    assert_eq!(fs::read_dir(&env.home).unwrap().count(), 0);
}

#[test]
fn search_json_is_versioned_and_ignores_invalid_local_configuration() {
    let env = Env::new();
    env.write_manifest("invalid TOML [[[");
    let (url, server) = search_server(
        "200 OK",
        r#"{"skills":[{"name":"review","source":"example/repo","installs":12,"id":"ignored"}]}"#
            .into(),
    );
    let out = search_command(&env, &url, &["search", "review", "--json"]);
    assert!(out.status.success());
    assert!(out.stderr.is_empty());
    let value: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(value["schema_version"], 2);
    assert_eq!(value["command"], "search");
    assert_eq!(value["status"], "ok");
    assert_eq!(
        value["data"]["skills"][0]["install_command"],
        "skl add example/repo --skill=review"
    );
    server.join().unwrap();
    assert_eq!(
        fs::read_to_string(env.config("skills.toml")).unwrap(),
        "invalid TOML [[["
    );
    assert!(!env.home.join(".local").exists());
}

#[test]
fn search_distinguishes_empty_results_from_http_schema_and_unsafe_metadata_errors() {
    let env = Env::new();
    let cases = [
        ("200 OK", r#"{"skills":[]}"#.to_owned(), true),
        (
            "503 Service Unavailable",
            r#"{"skills":[]}"#.to_owned(),
            false,
        ),
        ("200 OK", "not json".into(), false),
        ("200 OK", r#"{"results":[]}"#.to_owned(), false),
        (
            "200 OK",
            r#"{"skills":[{"name":"safe","source":"x/y","installs":-1}]}"#.to_owned(),
            false,
        ),
        (
            "200 OK",
            r#"{"skills":[{"name":"safe","source":"x/y;bad","installs":1}]}"#.to_owned(),
            false,
        ),
        (
            "200 OK",
            r#"{"skills":[{"name":"\u001b[2J","source":"x/y","installs":1}]}"#.to_owned(),
            false,
        ),
        ("200 OK", " ".repeat(1024 * 1024 + 1), false),
    ];
    for (status, body, succeeds) in cases {
        let (url, server) = search_server(status, body);
        let out = search_command(&env, &url, &["search", "test", "--json"]);
        assert_eq!(out.status.success(), succeeds);
        assert!(out.stderr.is_empty());
        assert!(!out.stdout.contains(&0x1b));
        let value: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        assert_eq!(value["status"], if succeeds { "ok" } else { "error" });
        if !succeeds {
            assert!(value["data"].is_null());
            assert_eq!(value["diagnostics"][0]["code"], "runtime-error");
        }
        server.join().unwrap();
    }
    let (url, server) = search_server("200 OK", r#"{"skills":[]}"#.into());
    let out = search_command(&env, &url, &["search", "empty"]);
    assert!(out.status.success());
    assert_eq!(String::from_utf8(out.stdout).unwrap(), "No skills found.\n");
    server.join().unwrap();
    assert_eq!(fs::read_dir(&env.home).unwrap().count(), 0);
}

#[test]
fn search_rejects_invalid_options_before_network_and_reports_connection_failure() {
    let env = Env::new();
    for flags in [
        vec!["--limit", "0"],
        vec!["--limit", "101"],
        vec!["--owner", "bad/owner"],
    ] {
        let mut args = vec!["search", "test", "--json"];
        args.extend(flags);
        let out = search_command(&env, "not-an-endpoint", &args);
        assert!(!out.status.success());
        let value: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        assert_eq!(value["status"], "error");
        assert!(out.stderr.is_empty());
    }
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/api/search", listener.local_addr().unwrap());
    drop(listener);
    let out = search_command(&env, &url, &["search", "test", "--json"]);
    assert!(!out.status.success());
    let value: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert!(
        value["diagnostics"][0]["message"]
            .as_str()
            .unwrap()
            .contains("search request failed")
    );
}

#[test]
fn interactive_requires_tty_before_any_write_or_fetch() {
    let env = Env::new();
    let error = env.fails(&["add", "invalid/repo", "--interactive"]);
    assert!(error.contains("requires terminal"));
    assert!(!env.config("skills.toml").exists());
    assert!(!env.home.join(".local/share/skl").exists());
}

#[test]
fn agent_detection_is_read_only_and_ignores_shared_skill_directories() {
    let env = Env::new();
    fs::create_dir_all(env.home.join(".agents/skills")).unwrap();
    fs::create_dir_all(env.home.join(".config/agents/skills")).unwrap();
    assert!(env.ok(&["agents", "detect"]).contains("No recognized"));
    fs::create_dir_all(env.home.join(".codex")).unwrap();
    fs::create_dir_all(env.home.join(".claude")).unwrap();
    let found = env.ok(&["agents", "detect"]);
    assert!(found.contains("codex"));
    assert!(found.contains("claude-code"));
    assert!(!found.contains("cline"));
    assert!(!env.home.join(".local/share/skl").exists());
    assert!(!env.config("skills.toml").exists());
}

fn interactive_pty(env: &Env, args: &[&str], input: &[u8]) -> (std::process::ExitStatus, String) {
    use std::io::{Read, Write};
    use std::os::fd::FromRawFd;
    use std::process::Stdio;
    let (mut master_fd, mut slave_fd) = (-1, -1);
    assert_eq!(
        unsafe {
            libc::openpty(
                &mut master_fd,
                &mut slave_fd,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        },
        0
    );
    let mut master = unsafe { fs::File::from_raw_fd(master_fd) };
    let slave = unsafe { fs::File::from_raw_fd(slave_fd) };
    let mut reader = master.try_clone().unwrap();
    let output = std::thread::spawn(move || {
        let mut result = Vec::new();
        let mut buffer = [0; 1024];
        loop {
            match reader.read(&mut buffer) {
                Ok(0) | Err(_) => break,
                Ok(count) => result.extend_from_slice(&buffer[..count]),
            }
        }
        String::from_utf8_lossy(&result).into_owned()
    });
    let mut child = Command::new(env!("CARGO_BIN_EXE_skl"))
        .args(args)
        .current_dir(&env.home)
        .env("HOME", &env.home)
        .env_remove("XDG_CONFIG_HOME")
        .env_remove("XDG_DATA_HOME")
        .env_remove("SKL_CONFIG_DIR")
        .env_remove("SKL_DATA_DIR")
        .env_remove("SKL_PROFILE")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .stdin(Stdio::from(slave.try_clone().unwrap()))
        .stderr(Stdio::from(slave.try_clone().unwrap()))
        .stdout(Stdio::from(slave))
        .spawn()
        .unwrap();
    master.write_all(input).unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if std::time::Instant::now() > deadline {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("interactive command timed out");
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    };
    drop(master);
    (status, output.join().unwrap())
}

#[test]
fn interactive_pty_selects_skills_and_agents_and_cancels_without_writes() {
    for input in [b"q\n".as_slice(), b"1\nq\n", b"999\n", b"\x04"] {
        let env = Env::new();
        let url = env.repo("catalog", &[("one", "one"), ("two", "two")]);
        let (status, output) = interactive_pty(&env, &["add", &url, "--interactive"], input);
        assert!(!status.success(), "{output}");
        assert!(!env.config("skills.toml").exists());
        assert!(!env.home.join(".local/share/skl").exists());
        assert!(!env.claude("one").exists());
    }
    let env = Env::new();
    let url = env.repo("catalog", &[("one", "one"), ("two", "two")]);
    // Choose the second skill and the first registry agent (adal).
    let (status, output) = interactive_pty(&env, &["add", &url, "--interactive"], b"2\n1\n");
    assert!(status.success(), "{output}");
    assert!(env.home.join(".adal/skills/two").is_symlink());
    assert!(!env.claude("two").exists());
    assert!(!env.home.join(".adal/skills/one").exists());
    let (status, output) = interactive_pty(
        &env,
        &[
            "add",
            &url,
            "--interactive",
            "--skill",
            "one",
            "--agent",
            "codex",
        ],
        b"",
    );
    assert!(status.success(), "{output}");
    assert!(!output.contains("Choose numbers"));
    assert!(env.codex("one").is_symlink());
}

#[test]
fn interactive_project_collection_selects_nested_skill_and_project_only_agent() {
    let env = Env::new();
    let project = env.home.join("project");
    let sources = project.join("sources");
    write_skill(&sources, "root-name", "root");
    write_skill(
        &sources.join("different-directory"),
        "selected-name",
        "nested",
    );
    let listing = env.ok(&["--project", project.to_str().unwrap(), "agents", "list"]);
    let eve = listing
        .lines()
        .position(|line| line.starts_with("eve "))
        .unwrap()
        + 1;
    let input = format!("2\n{eve}\n");
    let (status, output) = interactive_pty(
        &env,
        &[
            "--project",
            project.to_str().unwrap(),
            "add",
            sources.to_str().unwrap(),
            "--interactive",
        ],
        input.as_bytes(),
    );
    assert!(status.success(), "{output}");
    let doc: toml::Table = fs::read_to_string(project.join("skills.toml"))
        .unwrap()
        .parse()
        .unwrap();
    assert_eq!(
        doc["skills"]["selected-name"]["path"].as_str(),
        Some("sources/different-directory")
    );
    assert!(doc["skills"].as_table().unwrap().get("root-name").is_none());
    assert!(project.join("agent/skills/selected-name").is_symlink());
    assert!(!project.join(".agents/skills/selected-name").exists());
    assert!(!env.config("skills.toml").exists());
    assert!(!env.claude("selected-name").exists());
}

#[test]
fn project_agent_detection_does_not_infer_global_or_shared_harnesses() {
    let env = Env::new();
    let project = env.home.join("project");
    fs::create_dir_all(project.join(".agents/skills")).unwrap();
    fs::create_dir_all(env.home.join(".claude")).unwrap();
    fs::create_dir_all(env.home.join(".codex")).unwrap();
    let args = ["--project", project.to_str().unwrap(), "agents", "detect"];
    assert!(env.ok(&args).contains("No recognized"));
    fs::create_dir_all(project.join("agent")).unwrap();
    fs::create_dir_all(project.join(".cursor")).unwrap();
    let output = env.ok(&args);
    assert!(output.contains("eve "));
    assert!(output.contains("cursor "));
    assert!(!output.contains("codex"));
    assert!(!output.contains("claude-code"));
    assert!(!output.contains("promptscript"));
    assert!(!project.join("skills.toml").exists());
    assert!(!env.home.join(".local/share/skl").exists());
}

#[test]
fn repository_groups_select_skills_and_inherit_overridable_defaults() {
    let e = Env::new();
    let url = e.repo("group", &[("a", "a"), ("b", "b")]);
    git(&e.remote("group"), &["tag", "v1"]);
    write_skill(&e.remote("group").join("b"), "b", "new b");
    commit(&e.remote("group"), "new b");
    e.write_manifest(&format!("[repositories.tools]\nsource = '{url}'\nref = 'v1'\nskills = ['a', 'b']\nagents = ['codex']\nprofiles = ['work']\n[skills]\nb = {{ repository = 'tools', ref = 'main', agents = ['claude-code'] }}\n"));
    e.ok(&["--profile", "work", "sync"]);
    assert!(e.codex("a").join("SKILL.md").is_file());
    assert!(!e.claude("a").exists());
    assert!(!e.codex("b").exists());
    assert!(
        fs::read_to_string(e.claude("b").join("SKILL.md"))
            .unwrap()
            .contains("new b")
    );
    let lock = fs::read_to_string(e.config("skills.lock")).unwrap();
    assert!(lock.contains("ref = \"v1\""));
    assert!(lock.contains("ref = \"main\""));
    e.ok(&["--profile", "work", "sync", "--locked"]);
    e.ok(&["sync"]);
    assert!(!e.codex("a").exists());
    assert!(!e.claude("b").exists());
    assert_eq!(fs::read_to_string(e.config("skills.lock")).unwrap(), lock);
}

#[test]
fn repository_wildcard_sync_discovers_new_members_without_updating_existing_pins() {
    let e = Env::new();
    let url = e.repo("group", &[("a", "a")]);
    e.write_manifest(&format!(
        "[repositories.tools]\nsource = '{url}'\nskills = '*'\n"
    ));
    e.ok(&["sync"]);
    let old_target = fs::read_link(e.codex("a")).unwrap();
    write_skill(&e.remote("group").join("a"), "a", "new a");
    write_skill(&e.remote("group").join("b"), "b", "new b");
    commit(&e.remote("group"), "add b and update a");
    e.ok(&["sync", "--locked"]);
    assert!(!e.codex("b").exists());
    e.ok(&["sync"]);
    assert_eq!(fs::read_link(e.codex("a")).unwrap(), old_target);
    assert!(e.codex("b").join("SKILL.md").is_file());
    e.ok(&["update", "a"]);
    assert_ne!(fs::read_link(e.codex("a")).unwrap(), old_target);
    assert!(!old_target.exists());
    let listing = e.ok(&["list", "--json"]);
    assert!(listing.contains("\"git\""));
    assert!(!listing.contains("\"Xcode\""));
    e.ok(&["info", "b"]);
    e.ok(&["doctor"]);
}

#[test]
fn repository_wildcard_locked_is_offline_and_checks_coverage_declaration() {
    let e = Env::new();
    let url = e.repo("group", &[("a", "a")]);
    let manifest = format!("[repositories.tools]\nsource = '{url}'\nskills = '*'\n");
    e.write_manifest(&manifest);
    assert!(e.fails(&["sync", "--locked"]).contains("coverage"));
    e.ok(&["sync"]);
    fs::remove_dir_all(e.remote("group")).unwrap();
    let lock = fs::read(e.config("skills.lock")).unwrap();
    e.ok(&["sync", "--locked"]);
    e.ok(&["list"]);
    e.ok(&["doctor"]);
    assert_eq!(fs::read(e.config("skills.lock")).unwrap(), lock);
    e.write_manifest(&format!("{manifest}ref = 'other'\n"));
    assert!(
        e.fails(&["sync", "--locked"])
            .contains("coverage is out of date")
    );
    assert_eq!(fs::read(e.config("skills.lock")).unwrap(), lock);
    e.write_manifest(&format!(
        "[repositories.tools]\nsource = '{url}'\nskills = ['a']\n"
    ));
    assert!(e.fails(&["sync", "--locked"]).contains("removed wildcard"));
}

#[test]
fn repository_discovery_failure_preserves_lock_links_and_store() {
    let e = Env::new();
    let url = e.repo("group", &[("a", "a")]);
    e.write_manifest(&format!(
        "[repositories.tools]\nsource = '{url}'\nskills = '*'\n"
    ));
    e.ok(&["sync"]);
    let lock = fs::read(e.config("skills.lock")).unwrap();
    let target = fs::read_link(e.codex("a")).unwrap();
    // Ambiguous names fail the entire discovery, never publish partial coverage.
    write_skill(&e.remote("group").join("duplicate"), "a", "duplicate");
    commit(&e.remote("group"), "ambiguous skill");
    assert!(e.fails(&["sync"]).contains("ambiguous"));
    assert_eq!(fs::read(e.config("skills.lock")).unwrap(), lock);
    assert_eq!(fs::read_link(e.codex("a")).unwrap(), target);
    fs::remove_dir_all(e.remote("group")).unwrap();
    assert!(e.fails(&["update"]).contains("previous install preserved"));
    assert_eq!(fs::read(e.config("skills.lock")).unwrap(), lock);
    assert!(target.join("SKILL.md").is_file());
}

#[test]
fn repository_wildcards_filter_profiles_preserve_locks_and_reconcile_removed_members() {
    let e = Env::new();
    let url = e.repo("group", &[("a", "a"), ("b", "b")]);
    e.write_manifest(&format!("[repositories.tools]\nsource = '{url}'\nskills = '*'\nprofiles = ['work']\nagents = ['codex']\n"));
    e.ok(&["sync"]);
    assert!(!e.codex("a").exists());
    e.ok(&["--profile", "work", "sync"]);
    let lock = fs::read(e.config("skills.lock")).unwrap();
    let a_target = fs::read_link(e.codex("a")).unwrap();
    assert!(!e.claude("a").exists());
    e.ok(&["sync", "--locked"]);
    assert_eq!(fs::read(e.config("skills.lock")).unwrap(), lock);
    assert!(!e.codex("a").exists());
    assert!(a_target.exists());
    let offline = e.remotes.join("offline");
    fs::rename(e.remote("group"), &offline).unwrap();
    e.ok(&["--profile", "work", "sync", "--locked"]);
    assert!(e.codex("a").join("SKILL.md").is_file());
    fs::rename(&offline, e.remote("group")).unwrap();
    fs::remove_dir_all(e.remote("group").join("b")).unwrap();
    commit(&e.remote("group"), "remove b");
    e.ok(&["--profile", "work", "sync"]);
    assert!(e.codex("a").exists());
    assert!(!e.codex("b").exists());
    e.write_manifest("[skills]\n");
    e.ok(&["sync"]);
    assert!(!e.codex("a").exists());
    assert!(!a_target.exists());
}

#[test]
fn repository_groups_detect_collisions_and_allow_explicit_same_group_overrides() {
    let e = Env::new();
    let url = e.repo("group", &[("a", "a")]);
    e.write_manifest(&format!(
        "[repositories.tools]\nsource = '{url}'\nskills = '*'\n[skills]\na = '{url}'\n"
    ));
    assert!(e.fails(&["sync"]).contains("conflicts"));
    assert!(!e.config("skills.lock").exists());
    assert!(!e.codex("a").exists());
    e.write_manifest(&format!("[repositories.tools]\nsource = '{url}'\nskills = '*'\n[skills]\na = {{ repository = 'tools', agents = ['claude-code'] }}\n"));
    e.ok(&["sync"]);
    assert!(e.claude("a").exists());
    assert!(!e.codex("a").exists());
    e.ok(&["sync", "--locked"]);
}

#[test]
fn repository_wildcard_dry_run_and_uncached_inspection_are_read_only() {
    let e = Env::new();
    let url = e.repo("group", &[("a", "a")]);
    e.write_manifest(&format!(
        "[repositories.tools]\nsource = '{url}'\nskills = '*'\n"
    ));
    let output = e.skl(&["list", "--json"]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("source-not-locked"));
    e.ok(&["sync", "--dry-run"]);
    assert!(!e.config("skills.lock").exists());
    assert!(!e.home.join(".local/share/skl").exists());
    assert!(!e.codex("a").exists());
    let plan = e.home.join("plan.toml");
    e.ok(&["update", "--plan", plan.to_str().unwrap()]);
    assert!(plan.is_file());
    assert!(!e.config("skills.lock").exists());
    assert!(!e.home.join(".local/share/skl").exists());
    assert!(!e.codex("a").exists());
}

#[test]
fn repository_wildcard_empty_selection_has_offline_coverage() {
    let e = Env::new();
    let url = e.repo("group", &[("a", "a")]);
    fs::remove_dir_all(e.remote("group").join("a")).unwrap();
    commit(&e.remote("group"), "empty collection");
    e.write_manifest(&format!(
        "[repositories.tools]\nsource = '{url}'\nskills = '*'\n"
    ));
    e.ok(&["sync"]);
    let repos = e.home.join(".local/share/skl/repos");
    let cached = fs::read_dir(&repos)
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    assert!(cached.join("HEAD").is_file());
    fs::remove_dir_all(e.remote("group")).unwrap();
    e.ok(&["sync", "--locked"]);
    e.ok(&["doctor"]);
    assert!(cached.join("HEAD").is_file());
    e.write_manifest("[skills]\n");
    e.ok(&["sync"]);
    assert!(!cached.exists());
}

#[test]
fn repository_alias_only_supports_saved_plans_and_rejects_invalid_options() {
    let e = Env::new();
    let url = e.repo("group", &[("a", "a")]);
    e.write_manifest(&format!(
        "[repositories.tools]\nsource = '{url}'\n[skills]\na = {{ repository = 'tools' }}\n"
    ));
    e.ok(&["sync"]);
    write_skill(&e.remote("group").join("a"), "a", "new a");
    commit(&e.remote("group"), "new a");
    let plan = e.home.join("plan.toml");
    e.ok(&["update", "--plan", plan.to_str().unwrap()]);
    e.ok(&["update", "--apply-plan", plan.to_str().unwrap()]);
    assert!(body(&e.codex("a")).contains("new a"));
    for invalid in [
        "[repositories.tools]\nsource = 'a/b'\nskills = 42\n",
        "[repositories.tools]\npath = 'local'\n",
        "[repositories.tools]\nsource = 'a/b'\nos = ['unknown']\n",
        "[repositories.tools]\nsource = 'a/b'\n[skills]\na = { repository = 'missing' }\n",
        "[repositories.tools]\nsource = 'a/b'\n[skills]\na = { repository = 'tools', source = 'a/c' }\n",
    ] {
        e.write_manifest(invalid);
        e.fails(&["sync"]);
    }
    e.write_manifest(&format!(
        "[repositories.tools]\nsource = 'file:///unavailable'\nskills = '*'\nos = ['{}']\n",
        other_os()
    ));
    e.ok(&["sync"]);
}

#[path = "../tools/update_homebrew.rs"]
mod release_homebrew;

#[test]
fn homebrew_checksums_preserve_standard_sha256_encoding() {
    let tmp = TempDir::new().unwrap();
    // SHA-256("abc"), independent of the dependency used by the implementation.
    // Includes single-digit bytes to catch missing zero padding.
    let hash = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
    for target in [
        "aarch64-apple-darwin",
        "x86_64-apple-darwin",
        "aarch64-unknown-linux-gnu",
        "x86_64-unknown-linux-gnu",
    ] {
        let archive = format!("skl-{target}.tar.gz");
        fs::write(tmp.path().join(&archive), b"abc").unwrap();
        fs::write(
            tmp.path().join(format!("{archive}.sha256")),
            format!("{hash}  {archive}\n"),
        )
        .unwrap();
    }
    let updated =
        release_homebrew::update(include_str!("../Formula/skl.rb"), "v99.0.0", tmp.path()).unwrap();
    assert_eq!(updated.matches(&format!("sha256 \"{hash}\"")).count(), 4);
}

#[test]
fn homebrew_release_updates_all_archives_and_preserves_formula_test() {
    use sha2::{Digest, Sha256};
    let tmp = TempDir::new().unwrap();
    let original = include_str!("../Formula/skl.rb");
    let targets = [
        "aarch64-apple-darwin",
        "x86_64-apple-darwin",
        "aarch64-unknown-linux-gnu",
        "x86_64-unknown-linux-gnu",
    ];
    for target in targets {
        let archive = format!("skl-{target}.tar.gz");
        fs::write(tmp.path().join(&archive), target).unwrap();
        let hash = Sha256::digest(target.as_bytes())
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        fs::write(
            tmp.path().join(format!("{archive}.sha256")),
            format!("{hash}  {archive}\n"),
        )
        .unwrap();
    }
    let updated = release_homebrew::update(original, "v99.0.0", tmp.path()).unwrap();
    assert_eq!(updated.matches("/v99.0.0/").count(), 4);
    for target in targets {
        let hash = Sha256::digest(target.as_bytes())
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        assert!(updated.contains(&format!("sha256 \"{hash}\"")));
    }
    assert_eq!(
        updated.split("  def install").nth(1),
        original.split("  def install").nth(1)
    );
    assert_eq!(
        release_homebrew::update(&updated, "v99.0.0", tmp.path()).unwrap(),
        updated
    );
    // Old and prerelease tags do not require artifacts or downgrade the tap.
    let missing = tmp.path().join("missing");
    for tag in ["v98.0.0", "v100.0.0-rc.1", "v100.0.0+build.1"] {
        assert_eq!(
            release_homebrew::update(&updated, tag, &missing).unwrap(),
            updated
        );
    }
    assert!(release_homebrew::update(original, "v99.0.0", &missing).is_err());
    assert!(release_homebrew::update(original, "v99.0", tmp.path()).is_err());
    assert!(
        release_homebrew::update(
            &original.replace("sha256", "missing"),
            "v99.0.0",
            tmp.path()
        )
        .is_err()
    );
    // A corrupt archive or incorrect sidecar filename fails the whole update.
    let archive = "skl-x86_64-unknown-linux-gnu.tar.gz";
    let path = tmp.path().join(archive);
    fs::write(&path, "corrupt").unwrap();
    assert!(release_homebrew::update(original, "v99.0.0", tmp.path()).is_err());
    let hash = Sha256::digest(b"corrupt")
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    fs::write(
        tmp.path().join(format!("{archive}.sha256")),
        format!("{hash}  wrong.tar.gz\n"),
    )
    .unwrap();
    assert!(release_homebrew::update(original, "v99.0.0", tmp.path()).is_err());
}

#[test]
fn completions_are_generated_without_configuration_or_side_effects() {
    let env = Env::new();
    // These inputs would fail path/config resolution if generation reached it.
    fs::create_dir_all(env.home.join(".config/skl")).unwrap();
    fs::write(env.home.join(".config/skl/skills.toml"), "invalid = [").unwrap();
    for (shell, marker) in [
        ("bash", "complete -F _skl"),
        ("zsh", "#compdef skl"),
        ("fish", "complete -c skl"),
    ] {
        let out = Command::new(env!("CARGO_BIN_EXE_skl"))
            .args([
                "--project",
                "/nonexistent/skl-completions-project",
                "completions",
                shell,
            ])
            .current_dir(&env.home)
            .env_remove("HOME")
            .env("SKL_CONFIG_DIR", env.home.join(".config/skl"))
            .env("SKL_DATA_DIR", env.home.join("data"))
            .env("SKL_PROFILE", "invalid/profile")
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(out.stderr.is_empty());
        let script = String::from_utf8(out.stdout).unwrap();
        assert!(
            script.contains(marker),
            "missing {shell} header/registration"
        );
        for flag in ["dry-run", "locked", "only-profile"] {
            assert!(
                script.contains(flag),
                "missing {flag} in {shell} completions"
            );
        }
        assert!(script.contains("configure"));
        assert!(script.contains("sync"));
        assert!(!env.home.join("data").exists());
        assert!(!env.home.join("skills.toml").exists());
    }
    assert_eq!(
        fs::read_to_string(env.home.join(".config/skl/skills.toml")).unwrap(),
        "invalid = ["
    );
}

#[test]
fn completions_require_a_supported_shell() {
    let env = Env::new();
    for args in [vec!["completions"], vec!["completions", "unsupported"]] {
        let out = env.skl(&args);
        assert!(!out.status.success());
        assert!(out.stdout.is_empty());
        assert!(String::from_utf8_lossy(&out.stderr).contains("<SHELL>"));
    }
    assert!(!env.home.join(".local").exists());
    assert!(!env.home.join(".config").exists());
}

#[test]
fn repository_exclusions_filter_complete_coverage_and_allow_explicit_overrides() {
    let e = Env::new();
    let url = e.repo("group", &[("a", "a"), ("b", "b"), ("c", "c")]);
    e.write_manifest(&format!("[repositories.tools]\nsource = '{url}'\nskills = '*'\nexclude = ['a', 'b', 'future', 'a']\n[skills]\na = {{ repository = 'tools', agents = ['codex'] }}\nb = {{ source = '{url}', subdir = 'b', agents = ['claude-code'] }}\n"));
    e.ok(&["sync"]);
    assert!(e.codex("a").exists());
    assert!(!e.claude("a").exists());
    assert!(e.claude("b").exists());
    assert!(!e.codex("b").exists());
    assert!(e.codex("c").exists());
    let lock: toml::Value =
        toml::from_str(&fs::read_to_string(e.config("skills.lock")).unwrap()).unwrap();
    assert_eq!(
        lock["sources"]["tools"]["members"]
            .as_table()
            .unwrap()
            .len(),
        3
    );
    write_skill(&e.remote("group").join("future"), "future", "later");
    commit(&e.remote("group"), "add excluded future skill");
    e.ok(&["sync"]);
    assert!(!e.codex("future").exists());
    fs::remove_dir_all(e.remote("group")).unwrap();
    e.ok(&["sync", "--locked"]);
    e.ok(&["doctor"]);
    e.ok(&["list", "--json"]);
}

#[test]
fn repository_exclusions_reconcile_safely_and_keep_dry_runs_read_only() {
    let e = Env::new();
    let url = e.repo("group", &[("a", "a"), ("b", "b")]);
    let manifest = format!("[repositories.tools]\nsource = '{url}'\nskills = '*'\n");
    e.write_manifest(&manifest);
    e.ok(&["sync"]);
    let lock = fs::read(e.config("skills.lock")).unwrap();
    let state_path = e.home.join(".local/share/skl/state.toml");
    let state = fs::read(&state_path).unwrap();
    let target = fs::read_link(e.codex("a")).unwrap();
    // Replacing an owned link with user content must preserve that content.
    fs::remove_file(e.claude("a")).unwrap();
    fs::create_dir(e.claude("a")).unwrap();
    fs::write(e.claude("a").join("mine"), "user").unwrap();
    e.write_manifest(&format!("{manifest}exclude = ['a']\n"));
    e.fails(&["sync", "--locked"]);
    assert_eq!(fs::read(e.config("skills.lock")).unwrap(), lock);
    assert_eq!(fs::read_link(e.codex("a")).unwrap(), target);
    e.ok(&["sync", "--dry-run"]);
    assert_eq!(fs::read(e.config("skills.lock")).unwrap(), lock);
    assert_eq!(fs::read(&state_path).unwrap(), state);
    assert_eq!(fs::read_link(e.codex("a")).unwrap(), target);
    e.ok(&["sync"]);
    assert!(!e.codex("a").exists());
    assert_eq!(
        fs::read_to_string(e.claude("a").join("mine")).unwrap(),
        "user"
    );
    assert!(e.codex("b").exists());
    let lock = fs::read(e.config("skills.lock")).unwrap();
    fs::remove_dir_all(e.remote("group")).unwrap();
    e.ok(&["sync", "--locked"]);
    assert_eq!(fs::read(e.config("skills.lock")).unwrap(), lock);
}

#[test]
fn remove_wildcard_members_and_overrides_records_exclusions_atomically() {
    let e = Env::new();
    let url = e.repo("group", &[("a", "a"), ("b", "b")]);
    let manifest = format!(
        "# keep this comment\n[repositories.tools]\nsource = '{url}'\nskills = '*'\n[skills]\na = {{ repository = 'tools', agents = ['codex'] }} # override\n"
    );
    e.write_manifest(&manifest);
    e.ok(&["sync"]);
    e.fails(&["remove", "a", "missing"]);
    assert_eq!(
        fs::read_to_string(e.config("skills.toml")).unwrap(),
        manifest
    );
    assert!(e.codex("a").exists());
    // Atomic writes follow a dotfile symlink while preserving comments.
    let dotfile = e.home.join("skills.toml");
    fs::rename(e.config("skills.toml"), &dotfile).unwrap();
    std::os::unix::fs::symlink(&dotfile, e.config("skills.toml")).unwrap();
    e.ok(&["remove", "a", "b"]);
    assert!(
        fs::symlink_metadata(e.config("skills.toml"))
            .unwrap()
            .file_type()
            .is_symlink()
    );
    let text = fs::read_to_string(&dotfile).unwrap();
    assert!(text.contains("# keep this comment"));
    let doc: toml::Value = toml::from_str(&text).unwrap();
    assert_eq!(
        doc["repositories"]["tools"]["exclude"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert!(!e.codex("a").exists());
    assert!(!e.codex("b").exists());
    e.ok(&["sync"]);
    assert!(!e.codex("a").exists());
    assert!(!e.codex("b").exists());
    e.ok(&["sync", "--locked"]);
}

#[test]
fn repository_exclusions_validate_and_disabled_groups_remain_offline() {
    let e = Env::new();
    for config in [
        "skills = '*'\nexclude = 'a'",
        "skills = '*'\nexclude = [42]",
        "skills = '*'\nexclude = ['../a']",
        "skills = ['a']\nexclude = ['a']",
        "exclude = []",
    ] {
        e.write_manifest(&format!("[repositories.tools]\nsource = 'a/b'\n{config}\n"));
        e.fails(&["list"]);
        assert!(!e.config("skills.lock").exists());
    }
    let url = e.repo("group", &[("a", "a"), ("b", "b")]);
    e.write_manifest(&format!("[repositories.tools]\nsource = '{url}'\nskills = '*'\nexclude = ['a']\nprofiles = ['work']\n"));
    e.ok(&["--profile", "work", "sync"]);
    let lock = fs::read(e.config("skills.lock")).unwrap();
    fs::remove_dir_all(e.remote("group")).unwrap();
    e.ok(&["sync"]);
    assert!(!e.codex("b").exists());
    assert_eq!(fs::read(e.config("skills.lock")).unwrap(), lock);
    e.ok(&["--profile", "work", "sync", "--locked"]);
    assert!(e.codex("b").exists());
    assert!(!e.codex("a").exists());
}

#[test]
fn remove_undiscovered_wildcard_override_handles_inline_tables() {
    let e = Env::new();
    let url = e.repo("group", &[("a", "a"), ("b", "b")]);
    e.write_manifest(&format!("# inline declarations\nrepositories = {{ tools = {{ source = '{url}', skills = '*' }} }}\nskills = {{ a = {{ repository = 'tools' }} }}\n"));
    let before = fs::read(e.config("skills.toml")).unwrap();
    assert!(e.fails(&["remove", "b"]).contains("previous `skl sync`"));
    assert_eq!(fs::read(e.config("skills.toml")).unwrap(), before);
    e.ok(&["remove", "a"]);
    let text = fs::read_to_string(e.config("skills.toml")).unwrap();
    assert!(text.contains("# inline declarations"));
    let doc: toml::Value = toml::from_str(&text).unwrap();
    assert!(doc["skills"].as_table().unwrap().is_empty());
    assert_eq!(
        doc["repositories"]["tools"]["exclude"][0].as_str(),
        Some("a")
    );
    assert!(!e.codex("a").exists());
    assert!(e.codex("b").exists());
}

#[test]
fn add_joins_a_declared_source_list_like_a_workspace_dependency() {
    let e = Env::new();
    let skills: Vec<_> = ["a", "b", "c", "d", "e", "f", "g"]
        .map(|name| (format!("skills/{name}"), name))
        .into_iter()
        .collect();
    let mut layout: Vec<_> = skills
        .iter()
        .map(|(dir, name)| (dir.as_str(), *name))
        .collect();
    layout.push((".hidden/f", "f"));
    let url = e.repo("catalog", &layout);
    git(&e.remote("catalog"), &["branch", "next"]);
    fs::write(
        e.home.join(".gitconfig"),
        format!(
            "[url \"{url}\"]\n insteadOf = https://github.com/acme/catalog.git\n insteadOf = https://github.com/acme/catalog\n"
        ),
    )
    .unwrap();
    e.write_manifest(
        "# tools\n[sources.tools]\nsource = 'acme/catalog'\nskills = [\n  'b',\n  'd', # keep d\n]\nagents = ['codex']\n\n[skills]\n",
    );
    // A tree URL on the default branch joins a group that follows it, and the
    // name finds its directory, so neither ref nor subdir is repeated.
    let out = e.ok(&[
        "add",
        "https://github.com/acme/catalog/tree/main/skills/a",
        "--no-sync",
    ]);
    assert!(out.contains("Added a to source `tools`, inheriting agents = [\"codex\"]"));
    e.ok(&["add", "acme/catalog", "-s", "c", "-s", "c", "--no-sync"]);
    e.ok(&[
        "add",
        "acme/catalog",
        "--subdir",
        "skills/e",
        "--agent",
        "codex",
        "--no-sync",
    ]);
    let text = fs::read_to_string(e.config("skills.toml")).unwrap();
    assert!(
        text.contains("skills = [\n  \"a\",\n  'b',\n  \"c\",\n  'd', # keep d\n  \"e\",\n]"),
        "{text}"
    );
    assert!(text.starts_with("# tools\n"));
    let before = text.clone();
    assert!(
        e.ok(&["add", "acme/catalog@a", "--no-sync"])
            .contains("already in the manifest")
    );
    assert_eq!(fs::read_to_string(e.config("skills.toml")).unwrap(), before);
    // Settings a member doesn't inherit become a `source` override.
    let out = e.ok(&[
        "add",
        "acme/catalog",
        "--subdir",
        ".hidden/f",
        "--agent",
        "claude-code",
        "--no-sync",
    ]);
    assert!(out.contains("Added f to source `tools`\n"), "{out}");
    e.ok(&["add", "acme/catalog@g", "--ref", "next", "--no-sync"]);
    let text = fs::read_to_string(e.config("skills.toml")).unwrap();
    let doc: toml::Value = toml::from_str(&text).unwrap();
    let list: Vec<_> = doc["sources"]["tools"]["skills"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    // Members with their own settings appear only as entries, never also listed.
    assert_eq!(list, ["a", "b", "c", "d", "e"]);
    let entries = doc["skills"].as_table().unwrap();
    assert_eq!(entries.keys().collect::<Vec<_>>(), ["f", "g"]);
    assert_eq!(entries["f"]["source"].as_str(), Some("tools"));
    assert_eq!(entries["f"]["subdir"].as_str(), Some(".hidden/f"));
    assert_eq!(entries["f"]["agents"][0].as_str(), Some("claude-code"));
    assert_eq!(entries["g"]["ref"].as_str(), Some("next"));
    assert!(entries["g"].get("subdir").is_none());

    e.ok(&["sync"]);
    for name in ["a", "b", "c", "d", "e", "g"] {
        assert!(e.codex(name).is_symlink(), "{name}");
        assert!(!e.claude(name).exists(), "{name}");
    }
    assert!(e.claude("f").is_symlink());
    assert!(!e.codex("f").exists());
    let lock = fs::read_to_string(e.config("skills.lock")).unwrap();
    assert!(lock.contains("subdir = \".hidden/f\""));
    assert!(lock.contains("ref = \"next\""));
    e.ok(&["sync", "--locked"]);
    // A removed name takes its own line and comment with it.
    e.ok(&["remove", "d", "f"]);
    let text = fs::read_to_string(e.config("skills.toml")).unwrap();
    assert!(
        text.contains("skills = [\n  \"a\",\n  'b',\n  \"c\",\n  \"e\",\n]"),
        "{text}"
    );
    let doc: toml::Value = toml::from_str(&text).unwrap();
    assert_eq!(
        doc["skills"].as_table().unwrap().keys().collect::<Vec<_>>(),
        ["g"]
    );
    assert!(!e.codex("d").exists());
    assert!(!e.claude("f").exists());
}

#[test]
fn add_inherits_a_source_ref_unless_given_its_own() {
    let e = Env::new();
    let url = e.repo("group", &[("a", "a")]);
    git(&e.remote("group"), &["tag", "v1"]);
    write_skill(&e.remote("group").join("b"), "b", "later");
    commit(&e.remote("group"), "add b");
    e.write_manifest(&format!(
        "[sources.tools]\nsource = '{url}'\nref = 'v1'\nskills = ['a']\n"
    ));
    let before = fs::read(e.config("skills.toml")).unwrap();
    assert!(
        e.fails(&["add", &url, "--skill", "b", "--no-sync"])
            .contains("no skill named `b`")
    );
    assert_eq!(fs::read(e.config("skills.toml")).unwrap(), before);
    e.ok(&["add", &url, "--skill", "b", "--ref", "main"]);
    let doc: toml::Value =
        toml::from_str(&fs::read_to_string(e.config("skills.toml")).unwrap()).unwrap();
    assert_eq!(
        doc["sources"]["tools"]["skills"].as_array().unwrap().len(),
        1
    );
    assert_eq!(doc["skills"]["b"]["source"].as_str(), Some("tools"));
    assert_eq!(doc["skills"]["b"]["ref"].as_str(), Some("main"));
    assert!(body(&e.codex("b")).contains("later"));
    assert!(e.codex("a").is_symlink());
}

#[test]
fn add_restores_wildcard_exclusions_and_references_alias_only_sources() {
    let e = Env::new();
    let url = e.repo("group", &[("a", "a"), ("b", "b")]);
    let other = e.repo("other", &[("x", "x")]);
    e.write_manifest(&format!(
        "[sources.all]\nsource = '{url}'\nskills = '*'\nexclude = ['a']\n\n[sources.refs]\nsource = '{other}'\n"
    ));
    let before = fs::read(e.config("skills.toml")).unwrap();
    assert!(
        e.ok(&["add", &url, "--skill", "b", "--no-sync"])
            .contains("already selected by source `all`")
    );
    assert_eq!(fs::read(e.config("skills.toml")).unwrap(), before);
    e.ok(&["sync"]);
    assert!(!e.codex("a").exists());
    assert!(
        e.ok(&["add", &url, "--skill", "b", "--no-sync"])
            .contains("already in the manifest")
    );
    e.ok(&["add", &url, "--skill", "a"]);
    assert!(e.codex("a").is_symlink());
    e.ok(&["add", &other]);
    assert!(e.codex("x").is_symlink());
    let doc: toml::Value =
        toml::from_str(&fs::read_to_string(e.config("skills.toml")).unwrap()).unwrap();
    assert!(
        doc["sources"]["all"]["exclude"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    // A bare reference takes the string form, like `name = "owner/repo"`.
    assert_eq!(doc["skills"]["x"].as_str(), Some("refs"));
    assert!(doc["sources"]["refs"].get("skills").is_none());
    e.ok(&["sync", "--locked"]);
    e.ok(&["configure", "b", "--agent", "claude-code"]);
    assert!(e.claude("b").is_symlink());
    assert!(!e.codex("b").exists());
    let doc: toml::Value =
        toml::from_str(&fs::read_to_string(e.config("skills.toml")).unwrap()).unwrap();
    assert_eq!(doc["skills"]["b"]["source"].as_str(), Some("all"));
}

#[test]
fn skills_name_declared_sources_with_source_and_older_spellings_still_work() {
    let e = Env::new();
    let url = e.repo("group", &[("a", "a"), ("b", "b"), ("c", "c"), ("d", "d")]);
    let other = e.repo("other", &[("x", "x")]);
    let legacy = e.repo("legacy", &[("p", "p"), ("q", "q")]);
    // `[repositories]` and `repository = "alias"` from 0.2.0 sit beside `[sources]`,
    // the string form, and `source = "alias"`.
    e.write_manifest(&format!(
        "[sources.team]\nsource = '{url}'\nskills = ['a']\n\n[repositories.refs]\nsource = '{other}'\n\n[repositories.old]\nsource = '{legacy}'\nskills = ['p']\n\n[skills]\nb = 'team'\nc = {{ source = 'team', agents = ['codex'] }}\nd = {{ repository = 'team', agents = ['claude-code'] }}\nx = 'refs'\n"
    ));
    e.ok(&["sync"]);
    assert!(e.codex("a").is_symlink() && e.claude("a").is_symlink());
    assert!(e.codex("b").is_symlink() && e.claude("b").is_symlink());
    assert!(e.codex("c").is_symlink() && !e.claude("c").exists());
    assert!(e.claude("d").is_symlink() && !e.codex("d").exists());
    assert!(e.codex("x").is_symlink() && e.codex("p").is_symlink());
    e.ok(&["sync", "--locked"]);
    // `add` joins a source wherever it is declared.
    assert!(
        e.ok(&["add", &legacy, "--skill", "q", "--no-sync"])
            .contains("Added q to source `old`")
    );
    let doc: toml::Value =
        toml::from_str(&fs::read_to_string(e.config("skills.toml")).unwrap()).unwrap();
    assert_eq!(doc["repositories"]["old"]["skills"][1].as_str(), Some("q"));
    // A bare reference gains settings as a table and returns to the string form.
    e.ok(&["configure", "x", "--agent", "claude-code", "--no-sync"]);
    let text = fs::read_to_string(e.config("skills.toml")).unwrap();
    assert!(
        text.contains("x = { source = \"refs\", agents = [\"claude-code\"] }"),
        "{text}"
    );
    e.ok(&["configure", "x", "--default-agents", "--no-sync"]);
    let text = fs::read_to_string(e.config("skills.toml")).unwrap();
    assert!(text.contains("\nx = \"refs\"\n"), "{text}");
    e.ok(&["remove", "b"]);
    assert!(!e.codex("b").exists());

    for (manifest, error) in [
        (
            format!("[sources.team]\nsource = '{url}'\n[repositories.team]\nsource = '{url}'\n"),
            "declare source `team` once",
        ),
        (
            "[xcode]\nskills = '*'\n[repositories.xcode]\napp = 'xcode'\n".into(),
            "`[xcode]` and `[repositories.xcode]` both exist",
        ),
        (
            "[skills]\ny = 'nope'\n".into(),
            "skill `y` refers to unknown source `nope`",
        ),
        (
            format!(
                "[sources.team]\nsource = '{url}'\n[skills]\ny = {{ source = 'team', path = '/tmp' }}\n"
            ),
            "can't also set source or path",
        ),
        (
            format!(
                "[sources.team]\nsource = '{url}'\n[skills]\ny = {{ repository = 'team', source = '{url}' }}\n"
            ),
            "can't also set source or path",
        ),
    ] {
        e.write_manifest(&manifest);
        assert!(e.fails(&["list"]).contains(error), "{manifest}");
    }
}

#[test]
fn add_inserts_into_sorted_skills_tables() {
    let e = Env::new();
    let url = e.repo("catalog", &[("b", "b"), ("c", "c"), ("d", "d"), ("z", "z")]);
    e.write_manifest(&format!("[skills]\nb = '{url}' # first\nd = '{url}'\n"));
    e.ok(&["add", &url, "--skill", "c", "--no-sync"]);
    let text = fs::read_to_string(e.config("skills.toml")).unwrap();
    let keys: Vec<_> = text
        .lines()
        .filter_map(|line| line.split_once(" = ").map(|(key, _)| key))
        .collect();
    assert_eq!(keys, ["b", "c", "d"]);
    assert!(text.contains("# first"));
    // An unsorted table keeps its order and gains the new entry last.
    e.write_manifest(&format!("[skills]\nd = '{url}'\nb = '{url}'\n"));
    e.ok(&["add", &url, "--skill", "c", "--no-sync"]);
    let text = fs::read_to_string(e.config("skills.toml")).unwrap();
    let keys: Vec<_> = text
        .lines()
        .filter_map(|line| line.split_once(" = ").map(|(key, _)| key))
        .collect();
    assert_eq!(keys, ["d", "b", "c"]);
}

#[test]
fn configure_and_remove_source_list_members() {
    let e = Env::new();
    let url = e.repo("group", &[("a", "a"), ("b", "b")]);
    e.write_manifest(&format!(
        "[sources.tools]\nsource = '{url}'\nskills = ['a', 'b'] # list\n"
    ));
    e.ok(&["sync"]);
    // An override moves the name out of the list, so it appears once.
    e.ok(&["configure", "b", "--agent", "codex"]);
    let text = fs::read_to_string(e.config("skills.toml")).unwrap();
    assert!(text.contains("skills = ['a'] # list"), "{text}");
    assert!(text.contains("b = { source = \"tools\", agents = [\"codex\"] }"));
    assert!(e.codex("b").is_symlink());
    assert!(!e.claude("b").exists());
    // Resetting the last override moves it back.
    e.ok(&["configure", "b", "--default-agents"]);
    let text = fs::read_to_string(e.config("skills.toml")).unwrap();
    assert!(text.contains("skills = ['a', \"b\"] # list"), "{text}");
    let doc: toml::Value = toml::from_str(&text).unwrap();
    assert!(doc["skills"].get("b").is_none());
    assert!(e.claude("b").is_symlink());
    // Resetting a listed member that has no override changes nothing.
    e.ok(&["configure", "a", "--default-agents"]);
    assert_eq!(fs::read_to_string(e.config("skills.toml")).unwrap(), text);
    // A legacy entry that also stays listed settles into one place.
    e.write_manifest(&format!(
        "[sources.tools]\nsource = '{url}'\nskills = ['a', 'b']\n[skills]\nb = {{ source = 'tools', agents = ['codex'] }}\n"
    ));
    e.ok(&["configure", "b", "--os", "macos", "--no-sync"]);
    let doc: toml::Value =
        toml::from_str(&fs::read_to_string(e.config("skills.toml")).unwrap()).unwrap();
    assert_eq!(
        doc["sources"]["tools"]["skills"].as_array().unwrap().len(),
        1
    );
    assert_eq!(doc["skills"]["b"]["os"][0].as_str(), Some("macos"));
    e.ok(&["remove", "a"]);
    let text = fs::read_to_string(e.config("skills.toml")).unwrap();
    assert!(text.contains("skills = []"), "{text}");
    assert!(!e.codex("a").exists());
    e.ok(&["remove", "b"]);
    let doc: toml::Value =
        toml::from_str(&fs::read_to_string(e.config("skills.toml")).unwrap()).unwrap();
    assert!(doc["skills"].get("b").is_none());
    assert!(!e.codex("b").exists());
    assert!(e.fails(&["remove", "b"]).contains("isn't in the manifest"));
}

#[test]
fn add_only_joins_non_github_repositories_written_the_same_way() {
    let e = Env::new();
    let plain = e.repo("catalog", &[("a", "a")]);
    let bare = e.repo("catalog.git", &[("a", "a"), ("b", "b")]);
    e.write_manifest(&format!(
        "[sources.tools]\nsource = '{bare}'\nskills = ['b']\n"
    ));
    e.ok(&["add", &plain, "--skill", "a", "--no-sync"]);
    let doc: toml::Value =
        toml::from_str(&fs::read_to_string(e.config("skills.toml")).unwrap()).unwrap();
    assert_eq!(doc["skills"]["a"].as_str(), Some(plain.as_str()));
    assert_eq!(
        doc["sources"]["tools"]["skills"].as_array().unwrap().len(),
        1
    );
    // A source declared by absolute path is that Git repository, not a local skill.
    let path = e.remote("catalog.git");
    e.write_manifest(&format!(
        "[sources.tools]\nsource = '{}'\nskills = ['b']\n",
        path.display()
    ));
    let out = e.ok(&["add", path.to_str().unwrap(), "--skill", "a", "--no-sync"]);
    assert!(out.contains("Added a to source `tools`"), "{out}");
    let text = fs::read_to_string(e.config("skills.toml")).unwrap();
    assert!(text.contains("skills = [\"a\", 'b']"), "{text}");
    assert!(!text.contains("[skills]"), "{text}");
}

#[test]
fn configure_ignores_wildcard_coverage_from_a_previous_source() {
    let e = Env::new();
    let first = e.repo("first", &[("old", "old")]);
    let second = e.repo("second", &[("x", "x")]);
    e.write_manifest(&format!(
        "[sources.tools]\nsource = '{first}'\nskills = '*'\n"
    ));
    e.ok(&["sync"]);
    e.write_manifest(&format!(
        "[sources.tools]\nsource = '{second}'\nskills = '*'\n"
    ));
    let before = fs::read(e.config("skills.toml")).unwrap();
    assert!(
        e.fails(&["configure", "old", "--agent", "codex", "--no-sync"])
            .contains("skl sync")
    );
    assert_eq!(fs::read(e.config("skills.toml")).unwrap(), before);
}

#[test]
fn add_by_directory_name_to_a_wildcard_writes_a_reference() {
    let e = Env::new();
    let url = e.repo("group", &[("lint", "linter"), ("b", "b")]);
    e.write_manifest(&format!(
        "[sources.tools]\nsource = '{url}'\nskills = '*'\nexclude = ['linter']\n"
    ));
    e.ok(&["sync"]);
    e.ok(&["add", &url, "--skill", "lint"]);
    let doc: toml::Value =
        toml::from_str(&fs::read_to_string(e.config("skills.toml")).unwrap()).unwrap();
    assert_eq!(doc["skills"]["lint"].as_str(), Some("tools"));
    assert!(e.codex("lint").is_symlink());
    assert!(!e.codex("linter").exists());
}

#[test]
fn resetting_a_wildcard_member_drops_its_entry() {
    let e = Env::new();
    let url = e.repo("group", &[("alpha", "alpha"), ("beta", "beta")]);
    e.write_manifest(&format!("[sources.team]\nsource = '{url}'\nskills = '*'\n"));
    e.ok(&["sync"]);
    e.ok(&["configure", "alpha", "--agent", "claude-code"]);
    assert!(!e.codex("alpha").exists());
    // The wildcard selects alpha again, so no `alpha = "team"` pins it.
    e.ok(&["configure", "alpha", "--default-agents"]);
    let doc: toml::Value =
        toml::from_str(&fs::read_to_string(e.config("skills.toml")).unwrap()).unwrap();
    assert!(doc["skills"].get("alpha").is_none());
    assert!(e.codex("alpha").is_symlink());
    // An excluded member keeps its reference, which is what selects it.
    e.write_manifest(&format!(
        "[sources.team]\nsource = '{url}'\nskills = '*'\nexclude = ['beta']\n[skills]\nbeta = {{ source = 'team', agents = ['codex'] }}\n"
    ));
    e.ok(&["configure", "beta", "--default-agents"]);
    let doc: toml::Value =
        toml::from_str(&fs::read_to_string(e.config("skills.toml")).unwrap()).unwrap();
    assert_eq!(doc["skills"]["beta"].as_str(), Some("team"));
    assert!(e.codex("beta").is_symlink());
}

#[test]
fn adopt_snapshots_without_claiming_original_and_force_backs_it_up() {
    let env = Env::new();
    let original = env.home.join(".claude/skills/review");
    write_skill(&original, "review", "original");
    fs::write(original.join("notes.txt"), "keep me").unwrap();
    let out = env.skl(&["adopt", "review", "--from", "claude-code"]);
    assert!(String::from_utf8_lossy(&out.stdout).contains("existing paths remain conflicts"));
    let snapshot = env.config("local/review");
    assert_eq!(
        fs::read_to_string(snapshot.join("notes.txt")).unwrap(),
        "keep me"
    );
    assert!(
        !fs::symlink_metadata(&original)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert!(
        fs::read_link(env.home.join(".agents/skills/review"))
            .unwrap()
            .ends_with("local/review")
    );
    fs::write(original.join("notes.txt"), "changed").unwrap();
    assert_eq!(
        fs::read_to_string(snapshot.join("notes.txt")).unwrap(),
        "keep me"
    );
    env.ok(&["sync", "--force"]);
    assert_eq!(fs::read_link(&original).unwrap(), snapshot);
    assert!(
        fs::read_to_string(env.config("skills.toml"))
            .unwrap()
            .contains("local/review")
    );
    env.fails(&["adopt", "review", "--from", "claude-code"]);
}

#[test]
fn adopt_dry_run_and_invalid_sources_do_not_write() {
    let env = Env::new();
    let original = env.home.join(".claude/skills/review");
    write_skill(&original, "review", "original");
    env.ok(&["adopt", "review", "--from", "claude-code", "--dry-run"]);
    assert!(!env.config("skills.toml").exists());
    assert!(!env.config("local").exists());
    assert!(!env.home.join(".local/share/skl").exists());
    std::os::unix::fs::symlink("missing", original.join("escape")).unwrap();
    assert!(
        env.fails(&["adopt", "review", "--from", "claude-code"])
            .contains("nested symlinks")
    );
    assert!(!env.config("skills.toml").exists());
    assert!(!env.config("local").exists());
}

#[test]
fn adopt_custom_symlink_source_preserves_link_and_manifest_comments() {
    let env = Env::new();
    let source = env.home.join("external/review");
    write_skill(&source, "review", "original");
    let agent = env.home.join("custom");
    fs::create_dir_all(&agent).unwrap();
    std::os::unix::fs::symlink(&source, agent.join("review")).unwrap();
    let manifest = env.home.join("dotfiles.toml");
    fs::write(&manifest, "# retained\n").unwrap();
    fs::create_dir_all(env.config("")).unwrap();
    std::os::unix::fs::symlink(&manifest, env.config("skills.toml")).unwrap();
    env.ok(&[
        "adopt",
        "review",
        "--from",
        agent.to_str().unwrap(),
        "--no-sync",
    ]);
    assert_eq!(fs::read_link(agent.join("review")).unwrap(), source);
    assert!(
        fs::read_to_string(&manifest)
            .unwrap()
            .contains("# retained")
    );
    assert!(
        fs::symlink_metadata(env.config("skills.toml"))
            .unwrap()
            .file_type()
            .is_symlink()
    );
    fs::remove_dir_all(&source).unwrap();
    assert!(env.config("local/review/SKILL.md").is_file());
}

#[test]
fn adopt_rejects_duplicate_and_snapshot_destinations_and_rolls_back_failed_manifest() {
    let env = Env::new();
    write_skill(
        &env.home.join(".claude/skills/review"),
        "review",
        "original",
    );
    env.write_manifest("[skills]\nreview = { path = '~/other' }\n");
    assert!(
        env.fails(&["adopt", "review", "--from", "claude-code"])
            .contains("already in the manifest")
    );
    assert!(!env.config("local").exists());
    fs::remove_file(env.config("skills.toml")).unwrap();
    fs::create_dir_all(env.config("local/review")).unwrap();
    assert!(
        env.fails(&["adopt", "review", "--from", "claude-code"])
            .contains("already exists")
    );
    fs::remove_dir_all(env.config("local")).unwrap();
    // A directory at the manifest path fails before publishing a snapshot.
    fs::create_dir(env.config("skills.toml")).unwrap();
    env.fails(&["adopt", "review", "--from", "claude-code"]);
    assert!(!env.config("local").exists());
}

#[test]
fn adopt_project_snapshot_is_portable_and_scoped() {
    let env = Env::new();
    let project = env.home.join("project");
    write_skill(&project.join(".claude/skills/review"), "review", "original");
    env.ok(&[
        "--project",
        project.to_str().unwrap(),
        "adopt",
        "review",
        "--from",
        "claude-code",
        "--no-sync",
        "--agent",
        "codex",
    ]);
    assert!(project.join("local/review/SKILL.md").is_file());
    let text = fs::read_to_string(project.join("skills.toml")).unwrap();
    assert!(text.contains("local/review"));
    assert!(text.contains("codex"));
    assert!(!env.config("skills.toml").exists());
}

#[test]
fn adopt_rejects_snapshot_inside_source() {
    let env = Env::new();
    write_skill(&env.config(""), "skl", "original");
    assert!(
        env.fails(&["adopt", "skl", "--from", "~/.config", "--no-sync"])
            .contains("inside the installed source")
    );
    assert!(!env.config("local").exists());
}

#[test]
fn adopt_rejects_local_snapshot_parent_symlink_into_source() {
    let env = Env::new();
    let source = env.home.join(".claude/skills/review");
    write_skill(&source, "review", "original");
    fs::create_dir_all(env.config("")).unwrap();
    std::os::unix::fs::symlink(&source, env.config("local")).unwrap();
    assert!(
        env.fails(&["adopt", "review", "--from", "claude-code", "--no-sync"])
            .contains("must be a real directory")
    );
    assert!(!env.config("skills.toml").exists());
    assert_eq!(fs::read_dir(&source).unwrap().count(), 1);
}

#[test]
fn adopt_rejects_generated_and_unknown_wildcard_names_before_copying() {
    let env = Env::new();
    write_skill(
        &env.home.join(".claude/skills/review"),
        "review",
        "original",
    );
    env.write_manifest("[xcode]\nskills = ['review']\n");
    assert!(
        env.fails(&["adopt", "review", "--from", "claude-code", "--no-sync"])
            .contains("conflicts")
    );
    assert!(!env.config("local").exists());
    env.write_manifest("[xcode]\nskills = '*'\n");
    assert!(
        env.fails(&["adopt", "review", "--from", "claude-code", "--no-sync"])
            .contains("coverage is missing or stale")
    );
    assert!(!env.config("local").exists());
}

#[test]
fn adopt_rejects_cached_repository_membership_collision() {
    let env = Env::new();
    let url = env.repo("group", &[("review", "review")]);
    env.write_manifest(&format!(
        "agents = ['codex']\n[repositories.tools]\nsource = '{url}'\nskills = '*'\n"
    ));
    write_skill(
        &env.home.join(".claude/skills/review"),
        "review",
        "original",
    );
    assert!(
        env.fails(&["adopt", "review", "--from", "claude-code", "--no-sync"])
            .contains("coverage is missing or stale")
    );
    env.ok(&["sync"]);
    assert!(
        env.fails(&["adopt", "review", "--from", "claude-code", "--no-sync"])
            .contains("conflicts with another declaration")
    );
    assert!(!env.config("local").exists());
}

#[test]
fn adopt_rejects_missing_config_under_source_alias_without_writes() {
    let env = Env::new();
    let source = env.home.join(".claude/skills/review");
    write_skill(&source, "review", "original");
    let alias = env.home.join("alias");
    std::os::unix::fs::symlink(&source, &alias).unwrap();
    let config = alias.join("missing-config");
    let out = env.skl_with_env(
        &["adopt", "review", "--from", "claude-code", "--dry-run"],
        &[("SKL_CONFIG_DIR", config.as_os_str())],
    );
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("inside the installed source"));
    assert!(!config.exists());
    assert_eq!(fs::read_dir(&source).unwrap().count(), 1);
}

#[test]
fn adopt_allows_locally_excluded_cached_wildcard_member() {
    let env = Env::new();
    let url = env.repo("group", &[("review", "review")]);
    env.write_manifest(&format!(
        "agents = ['codex']\n[repositories.tools]\nsource = '{url}'\nskills = '*'\n"
    ));
    env.ok(&["sync"]);
    env.ok(&["remove", "review"]);
    write_skill(
        &env.home.join(".claude/skills/review"),
        "review",
        "local replacement",
    );
    env.ok(&["adopt", "review", "--from", "claude-code", "--no-sync"]);
    env.ok(&["sync"]);
    let manifest = fs::read_to_string(env.config("skills.toml")).unwrap();
    assert!(manifest.contains("exclude = [\"review\"]"));
    assert!(manifest.contains("local/review"));
    assert_eq!(
        fs::read_link(env.codex("review")).unwrap(),
        env.config("local/review")
    );
    assert!(
        fs::read_to_string(env.config("local/review/SKILL.md"))
            .unwrap()
            .contains("local replacement")
    );
    assert!(
        !fs::symlink_metadata(env.claude("review"))
            .unwrap()
            .file_type()
            .is_symlink()
    );
}

/// Persistent bytes, types, symlink targets and permissions; don't follow links.
fn verification_snapshot(root: &Path) -> Vec<(PathBuf, u32, Vec<u8>)> {
    use std::os::unix::fs::PermissionsExt;
    fn visit(root: &Path, path: &Path, out: &mut Vec<(PathBuf, u32, Vec<u8>)>) {
        let metadata = fs::symlink_metadata(path).unwrap();
        let bytes = if metadata.file_type().is_symlink() {
            fs::read_link(path)
                .unwrap()
                .as_os_str()
                .as_encoded_bytes()
                .to_vec()
        } else if metadata.is_file() {
            fs::read(path).unwrap()
        } else {
            Vec::new()
        };
        out.push((
            path.strip_prefix(root).unwrap().into(),
            metadata.permissions().mode(),
            bytes,
        ));
        if metadata.is_dir() {
            for entry in fs::read_dir(path).unwrap() {
                visit(root, &entry.unwrap().path(), out);
            }
        }
    }
    let mut out = Vec::new();
    visit(root, root, &mut out);
    out.sort();
    out
}

#[test]
fn verify_git_content_detects_resource_type_symlink_mode_and_membership_drift() {
    use std::os::unix::fs::{PermissionsExt, symlink};
    let e = Env::new();
    let url = e.repo("verification", &[("notes", "notes")]);
    let source = e.remote("verification").join("notes");
    fs::write(source.join("resource.txt"), "original resource").unwrap();
    fs::write(source.join("deleted.txt"), "delete me").unwrap();
    fs::write(source.join("run"), "executable content").unwrap();
    fs::set_permissions(source.join("run"), fs::Permissions::from_mode(0o755)).unwrap();
    fs::create_dir(source.join("nested")).unwrap();
    fs::write(source.join("nested/data"), "nested data").unwrap();
    symlink("resource.txt", source.join("alias")).unwrap();
    commit(&e.remote("verification"), "resources and types");
    e.write_manifest(&format!("[skills]\nnotes = '{url}'\n"));
    e.ok(&["sync"]);
    fs::remove_dir_all(e.remote("verification")).unwrap();
    let baseline = verification_snapshot(&e.home);
    assert!(e.ok(&["verify"]).contains("verified: notes"));
    assert_eq!(verification_snapshot(&e.home), baseline);
    let target = fs::read_link(e.codex("notes")).unwrap();
    fs::write(target.join("SKILL.md"), "altered skill").unwrap();
    fs::write(target.join("resource.txt"), "altered resource").unwrap();
    fs::remove_file(target.join("deleted.txt")).unwrap();
    fs::write(target.join("added.txt"), "unexpected").unwrap();
    fs::set_permissions(target.join("run"), fs::Permissions::from_mode(0o744)).unwrap();
    fs::remove_file(target.join("alias")).unwrap();
    symlink("added.txt", target.join("alias")).unwrap();
    // Replacing a directory with an external symlink must compare the link itself.
    fs::remove_dir_all(target.join("nested")).unwrap();
    symlink("/a/nonexistent/external/directory", target.join("nested")).unwrap();
    let before = verification_snapshot(&e.home);
    let result = e.skl(&["verify", "notes", "--json"]);
    assert!(!result.status.success());
    assert!(result.stderr.is_empty());
    let report: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(report["schema_version"], 2);
    assert_eq!(report["command"], "verify");
    assert_eq!(report["status"], "unhealthy");
    assert_eq!(report["data"]["skills"][0]["status"], "drift");
    let changes = report["data"]["skills"][0]["changes"].as_array().unwrap();
    for (path, kind) in [
        ("SKILL.md", "changed"),
        ("resource.txt", "changed"),
        ("deleted.txt", "deleted"),
        ("added.txt", "added"),
        ("run", "changed"),
        ("alias", "changed"),
        ("nested", "changed"),
        ("nested/data", "deleted"),
    ] {
        assert!(
            changes
                .iter()
                .any(|change| change["path"] == path && change["kind"] == kind),
            "missing {path}: {report}"
        );
    }
    assert_eq!(verification_snapshot(&e.home), before);
}

#[test]
fn verify_reports_missing_offline_baselines_and_rejects_root_symlinks() {
    use std::os::unix::fs::symlink;
    let e = Env::new();
    let url = e.repo("verification", &[("notes", "notes")]);
    e.write_manifest(&format!("[skills]\nnotes = '{url}'\n"));
    e.ok(&["sync"]);
    let target = fs::read_link(e.codex("notes")).unwrap();
    let moved = e.home.join("moved");
    fs::rename(&target, &moved).unwrap();
    symlink(&moved, &target).unwrap();
    let before = verification_snapshot(&e.home);
    let result = e.skl(&["verify"]);
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stdout).contains("symlink"));
    assert_eq!(verification_snapshot(&e.home), before);
    fs::remove_file(&target).unwrap();
    fs::rename(&moved, &target).unwrap();
    fs::remove_dir_all(e.home.join(".local/share/skl/repos")).unwrap();
    fs::remove_dir_all(e.remote("verification")).unwrap();
    let before = verification_snapshot(&e.home);
    let result = e.skl(&["verify", "--json"]);
    assert!(!result.status.success());
    let report: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(report["data"]["skills"][0]["status"], "unavailable");
    assert_eq!(report["diagnostics"][0]["code"], "baseline-unavailable");
    assert_eq!(verification_snapshot(&e.home), before);
}

#[test]
fn verify_skips_local_and_disabled_skills_and_reports_json_errors() {
    let e = Env::new();
    write_skill(&e.home.join("local"), "local", "unversioned");
    e.write_manifest("[skills]\nlocal = { path = '~/local' }\ninactive = { source = 'file:///unavailable', profiles = ['work'] }\n");
    let before = verification_snapshot(&e.home);
    let output = e.ok(&["verify", "--json"]);
    let report: serde_json::Value = serde_json::from_str(&output).unwrap();
    let skills = report["data"]["skills"].as_array().unwrap();
    assert_eq!(skills.len(), 2);
    assert!(skills.iter().all(|s| s["status"] == "skipped"));
    assert!(output.contains("no pinned content baseline"));
    assert!(
        !e.skl(&["--profile", "work", "verify", "inactive"])
            .status
            .success()
    );
    let result = e.skl(&["verify", "unknown", "--json"]);
    assert!(!result.status.success());
    assert!(result.stderr.is_empty());
    let error: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(error["status"], "error");
    assert_eq!(error["data"], serde_json::Value::Null);
    assert_eq!(verification_snapshot(&e.home), before);
}

#[test]
fn verify_respects_project_scope_and_selected_names() {
    let e = Env::new();
    let url = e.repo("verification", &[("a", "a"), ("b", "b")]);
    let project = e.home.join("project");
    fs::create_dir(&project).unwrap();
    fs::write(
        project.join("skills.toml"),
        format!("[skills]\na = '{url}'\nb = '{url}'\n"),
    )
    .unwrap();
    let project_arg = project.to_str().unwrap();
    e.ok(&["--project", project_arg, "sync"]);
    let bad = fs::read_link(project.join(".agents/skills/b")).unwrap();
    fs::write(bad.join("SKILL.md"), "drift").unwrap();
    let before = verification_snapshot(&e.home);
    e.ok(&["--project", project_arg, "verify", "a"]);
    let result = e.skl(&["--project", project_arg, "verify"]);
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stdout).contains("drift: b"));
    assert!(!e.config("skills.toml").exists());
    assert_eq!(verification_snapshot(&e.home), before);
}

#[test]
fn verify_xcode_digest_needs_no_tools_and_detects_content_changes() {
    let e = Env::new();
    e.write_manifest("[xcode]\nskills = '*'\nos = ['macos', 'linux']\n");
    e.export_xcode(&[("example", "original")], "Xcode test");
    assert!(e.xcode(&["sync"]).status.success());
    let target = fs::read_link(e.codex("example")).unwrap();
    let no_tools = e.home.join("no-tools");
    fs::create_dir(&no_tools).unwrap();
    let before = verification_snapshot(&e.home);
    let verified = e.skl_with_env(&["verify", "--json"], &[("PATH", no_tools.as_os_str())]);
    assert!(verified.status.success());
    let report: serde_json::Value = serde_json::from_slice(&verified.stdout).unwrap();
    assert_eq!(report["data"]["skills"][0]["source_kind"], "app");
    assert_eq!(report["data"]["skills"][0]["status"], "verified");
    assert_eq!(verification_snapshot(&e.home), before);
    fs::write(target.join("SKILL.md"), "altered").unwrap();
    let changed = e.skl_with_env(&["verify"], &[("PATH", no_tools.as_os_str())]);
    assert!(!changed.status.success());
    assert!(String::from_utf8_lossy(&changed.stdout).contains("drift: example"));
}

#[test]
fn verify_unavailable_tools_and_non_utf8_changes_are_structured_failures() {
    let e = Env::new();
    let url = e.repo("verification", &[("notes", "notes")]);
    e.write_manifest(&format!("[skills]\nnotes = '{url}'\n"));
    e.ok(&["sync"]);
    let no_tools = e.home.join("no-tools");
    fs::create_dir(&no_tools).unwrap();
    let before = verification_snapshot(&e.home);
    let unavailable = e.skl_with_env(&["verify", "--json"], &[("PATH", no_tools.as_os_str())]);
    assert!(!unavailable.status.success());
    assert!(unavailable.stderr.is_empty());
    let report: serde_json::Value = serde_json::from_slice(&unavailable.stdout).unwrap();
    assert_eq!(report["data"]["skills"][0]["status"], "unavailable");
    assert!(report["diagnostics"][0]["path"].is_string());
    assert_eq!(verification_snapshot(&e.home), before);
    // macOS filesystems reject invalid UTF-8 filenames; exercise the complete
    // runtime envelope on Linux and fallible serialization on all platforms below.
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::ffi::OsStrExt;
        let target = fs::read_link(e.codex("notes")).unwrap();
        fs::write(
            target.join(std::ffi::OsStr::from_bytes(b"non-utf8-\xff")),
            "added",
        )
        .unwrap();
        let result = e.skl(&["verify", "--json"]);
        assert!(!result.status.success());
        assert!(result.stderr.is_empty());
        let error: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
        assert_eq!(error["status"], "error");
        assert_eq!(error["diagnostics"][0]["code"], "runtime-error");
    }
}

#[test]
fn verify_undiscovered_wildcards_report_unavailable_without_creating_data() {
    let e = Env::new();
    e.write_manifest("[repositories.tools]\nsource = 'file:///unavailable'\nskills = '*'\n");
    let before = verification_snapshot(&e.home);
    let result = e.skl(&["verify", "--json"]);
    assert!(!result.status.success());
    let report: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(report["diagnostics"][0]["code"], "baseline-unavailable");
    assert_eq!(verification_snapshot(&e.home), before);
}

#[test]
fn verify_never_lazy_fetches_missing_promisor_objects() {
    let e = Env::new();
    let url = e.repo("verification", &[("notes", "notes")]);
    e.write_manifest(&format!("[skills]\nnotes = '{url}'\n"));
    e.ok(&["sync"]);
    let repo = fs::read_dir(e.home.join(".local/share/skl/repos"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    git(&repo, &["config", "extensions.partialClone", "origin"]);
    git(&repo, &["config", "remote.origin.url", &url]);
    git(&repo, &["config", "remote.origin.promisor", "true"]);
    fs::remove_dir_all(repo.join("objects")).unwrap();
    fs::create_dir_all(repo.join("objects/pack")).unwrap();
    fs::create_dir_all(repo.join("objects/info")).unwrap();
    // The remote remains reachable: fetching could restore the missing objects,
    // but verification must fail without modifying this partial cache.
    let before = verification_snapshot(&e.home);
    let result = e.skl(&["verify", "--json"]);
    assert!(!result.status.success());
    let report: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(report["data"]["skills"][0]["status"], "unavailable");
    assert_eq!(verification_snapshot(&e.home), before);
}

#[test]
fn saved_plan_freezes_wildcard_membership_and_pins() {
    let e = Env::new();
    let url = e.repo("wildplan", &[("a", "a"), ("gone", "gone")]);
    e.write_manifest(&format!(
        "[repositories.team]\nsource = '{url}'\nskills = '*'\n"
    ));
    e.ok(&["sync"]);
    fs::remove_dir_all(e.remote("wildplan").join("gone")).unwrap();
    write_skill(&e.remote("wildplan").join("b"), "b", "reviewed addition");
    write_skill(&e.remote("wildplan").join("a"), "a", "reviewed update");
    commit(&e.remote("wildplan"), "reviewed membership");
    let before = fs::read(e.config("skills.lock")).unwrap();
    e.ok(&["update", "--plan", "review.toml"]);
    assert_eq!(before, fs::read(e.config("skills.lock")).unwrap());
    assert!(!e.codex("b").exists());
    assert!(e.codex("gone").exists());
    let plan = fs::read_to_string(e.home.join("review.toml")).unwrap();
    assert!(plan.contains("+reviewed addition"));
    assert!(plan.contains("removed:gone"));
    fs::remove_dir_all(e.remote("wildplan").join("b")).unwrap();
    write_skill(
        &e.remote("wildplan").join("later"),
        "later",
        "unreviewed addition",
    );
    commit(&e.remote("wildplan"), "unreviewed membership");
    e.ok(&["update", "--apply-plan", "review.toml"]);
    assert!(body(&e.codex("a")).contains("reviewed update"));
    assert!(body(&e.codex("b")).contains("reviewed addition"));
    assert!(!e.codex("gone").exists());
    assert!(!e.codex("later").exists());
}

#[test]
fn saved_plan_xcode_embeds_reviewed_content_and_applies_without_export() {
    use std::os::unix::fs::PermissionsExt;
    let e = Env::new();
    e.write_manifest("[xcode]\nskills = '*'\nos = ['macos', 'linux']\n");
    e.export_xcode(&[("apple", "reviewed content")], "Xcode 27\nBuild reviewed");
    let script = e.remotes.join("export/apple/run");
    fs::write(&script, [0, 255, 1, 128]).unwrap();
    fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
    fs::create_dir(e.remotes.join("export/apple/empty")).unwrap();
    let out = e.xcode(&["update", "--plan", "review.toml"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(!e.home.join(".local/share/skl").exists());
    assert!(!e.config("skills.lock").exists());
    let original = fs::read_to_string(e.home.join("review.toml")).unwrap();
    assert!(original.contains("+reviewed content"));
    // A dry apply must not publish embedded snapshots or any other data.
    assert!(
        e.xcode(&["update", "--apply-plan", "review.toml", "--dry-run"])
            .status
            .success()
    );
    assert!(!e.home.join(".local/share/skl").exists());
    e.export_xcode(
        &[("apple", "changed after review"), ("later", "unreviewed")],
        "Xcode 28",
    );
    fs::write(e.remotes.join("fail"), "no export allowed").unwrap();
    // No xcrun on PATH is needed when applying the self-contained plan.
    e.ok(&["update", "--apply-plan", "review.toml"]);
    assert!(body(&e.codex("apple")).contains("reviewed content"));
    assert!(!body(&e.codex("apple")).contains("changed after review"));
    assert!(!e.codex("later").exists());
    assert_eq!(
        fs::read(e.codex("apple").join("run")).unwrap(),
        [0, 255, 1, 128]
    );
    assert_eq!(
        fs::metadata(e.codex("apple").join("run"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o755
    );
    assert!(e.codex("apple").join("empty").is_dir());
}

#[test]
fn saved_plan_xcode_tampering_and_generated_destinations_are_rejected() {
    let e = Env::new();
    e.write_manifest("[xcode]\nskills = '*'\nos = ['macos', 'linux']\nagents = ['~/generated']\n");
    e.export_xcode(&[("apple", "reviewed")], "Xcode 27");
    fs::create_dir_all(e.home.join("generated")).unwrap();
    assert!(
        !e.xcode(&["update", "--plan", "generated/review.toml"])
            .status
            .success()
    );
    assert!(!e.home.join("generated/review.toml").exists());
    assert!(
        e.xcode(&["update", "--plan", "review.toml"])
            .status
            .success()
    );
    let original = fs::read_to_string(e.home.join("review.toml")).unwrap();
    fs::write(
        e.home.join("review.toml"),
        original.replace("+reviewed", "+edited"),
    )
    .unwrap();
    assert!(
        e.fails(&["update", "--apply-plan", "review.toml"])
            .contains("checksum")
    );
    fs::write(e.home.join("review.toml"), original).unwrap();
    fs::rename(e.home.join("generated"), e.home.join("elsewhere")).unwrap();
    std::os::unix::fs::symlink(e.home.join("elsewhere"), e.home.join("generated")).unwrap();
    assert!(
        e.fails(&["update", "--apply-plan", "review.toml"])
            .contains("stale")
    );
}

#[test]
fn saved_plan_xcode_selected_updates_preserve_other_pins_and_review_removals() {
    let e = Env::new();
    e.write_manifest("[xcode]\nskills = '*'\nos = ['macos', 'linux']\n");
    e.export_xcode(
        &[("a", "old a"), ("b", "old b"), ("gone", "removed content")],
        "Xcode 27",
    );
    assert!(e.xcode(&["sync"]).status.success());
    e.export_xcode(&[("a", "new a"), ("b", "unselected b")], "Xcode 28");
    assert!(
        e.xcode(&["update", "a", "--plan", "selected.toml"])
            .status
            .success()
    );
    fs::remove_file(e.remotes.join("called")).unwrap();
    e.ok(&["update", "--apply-plan", "selected.toml"]);
    assert!(!e.remotes.join("called").exists());
    assert!(body(&e.codex("a")).contains("new a"));
    assert!(body(&e.codex("b")).contains("old b"));
    assert!(body(&e.codex("gone")).contains("removed content"));
    assert!(e.xcode(&["update", "--plan", "all.toml"]).status.success());
    let plan = fs::read_to_string(e.home.join("all.toml")).unwrap();
    assert!(plan.contains("removed:app:gone"));
    assert!(plan.contains("-removed content"));
    e.ok(&["update", "--apply-plan", "all.toml"]);
    assert!(!e.codex("gone").exists());
    assert!(body(&e.codex("b")).contains("unselected b"));
}

#[test]
fn saved_plan_keeps_an_app_source_exported_by_an_earlier_command() {
    let e = Env::new();
    let url = e.repo("onlygit", &[("a", "a")]);
    let manifest = format!(
        "[skills]\na = '{url}'\n[sources.xcode]\napp = 'xcode'\nskills = '*'\nos = ['macos', 'linux']\n"
    );
    e.write_manifest(&manifest);
    e.export_xcode(&[("apple", "v1")], "Xcode 27");
    assert!(e.xcode(&["sync"]).status.success());
    // A new timeout changes the export command, but this plan only updates `a`.
    e.write_manifest(&format!("{manifest}timeout = 60\n"));
    e.ok(&["update", "a", "--plan", "git.toml"]);
    e.ok(&["update", "--apply-plan", "git.toml"]);
    assert!(body(&e.codex("apple")).contains("v1"));
}

#[test]
fn saved_plan_disabled_or_unrelated_sources_do_not_export() {
    let e = Env::new();
    let url = e.repo("onlygit", &[("a", "a")]);
    e.write_manifest(&format!(
        "[skills]\na = {{source = '{url}'}}\n[xcode]\nskills = '*'\nos = ['macos', 'linux']\n"
    ));
    e.ok(&["update", "a", "--plan", "git.toml"]);
    assert!(!e.remotes.join("called").exists());
    e.ok(&["update", "--apply-plan", "git.toml"]);
    e.write_manifest(&format!("[repositories.disabled]\nsource = 'file:///unavailable'\nskills = '*'\nos = ['{}']\n[xcode]\nskills = '*'\nos = ['{}']\n", other_os(), other_os()));
    e.ok(&["update", "--plan", "disabled.toml"]);
    e.ok(&["update", "--apply-plan", "disabled.toml"]);
    assert!(!e.remotes.join("called").exists());
}

#[test]
fn saved_plan_xcode_missing_previous_content_fails_without_artifact() {
    let e = Env::new();
    e.write_manifest("[xcode]\nskills = ['a']\nos = ['macos', 'linux']\n");
    e.export_xcode(&[("a", "old a")], "Xcode 27");
    assert!(e.xcode(&["sync"]).status.success());
    fs::remove_dir_all(e.home.join(".local/share/skl/store")).unwrap();
    e.export_xcode(&[("a", "new a")], "Xcode 28");
    assert!(
        !e.xcode(&["update", "--plan", "missing.toml"])
            .status
            .success()
    );
    assert!(!e.home.join("missing.toml").exists());
}

#[test]
fn saved_plan_payload_tampering_is_rejected() {
    let e = Env::new();
    e.write_manifest("[xcode]\nskills = '*'\nos = ['macos', 'linux']\n");
    e.export_xcode(&[("a", "reviewed")], "Xcode 27");
    assert!(
        e.xcode(&["update", "--plan", "review.toml"])
            .status
            .success()
    );
    let original = fs::read_to_string(e.home.join("review.toml")).unwrap();
    for alteration in ["bytes", "path", "mode"] {
        let mut plan: toml::Value = toml::from_str(&original).unwrap();
        let nodes = plan["app_snapshots"]["a"]["nodes"].as_table_mut().unwrap();
        match alteration {
            "bytes" => {
                nodes.get_mut("SKILL.md").unwrap()["bytes"] =
                    toml::Value::Array(vec![toml::Value::Integer(0)])
            }
            "path" => {
                let node = nodes.remove("SKILL.md").unwrap();
                nodes.insert("../escape".into(), node);
            }
            "mode" => nodes.get_mut("SKILL.md").unwrap()["mode"] = toml::Value::Integer(0o104755),
            _ => unreachable!(),
        }
        fs::write(e.home.join("review.toml"), toml::to_string(&plan).unwrap()).unwrap();
        let err = e.fails(&["update", "--apply-plan", "review.toml"]);
        assert!(err.contains("checksum"), "{err}");
        assert!(!e.home.join(".local/share/skl/store").exists());
        assert!(!e.home.join("escape").exists());
    }
}

#[test]
fn saved_plan_wildcard_destinations_protect_aliases_and_reject_stale_paths() {
    let e = Env::new();
    let url = e.repo("wilddest", &[("a", "a")]);
    e.write_manifest(&format!(
        "[repositories.team]\nsource = '{url}'\nskills = '*'\nagents = ['~/generated']\n"
    ));
    fs::create_dir(e.home.join("generated")).unwrap();
    std::os::unix::fs::symlink(e.home.join("generated"), e.home.join("alias")).unwrap();
    assert!(
        e.fails(&["update", "--plan", "alias/review.toml"])
            .contains("outside")
    );
    assert!(!e.home.join("generated/review.toml").exists());
    e.ok(&["update", "--plan", "review.toml"]);
    fs::rename(e.home.join("generated"), e.home.join("moved")).unwrap();
    std::os::unix::fs::symlink(e.home.join("moved"), e.home.join("generated")).unwrap();
    assert!(
        e.fails(&["update", "--apply-plan", "review.toml"])
            .contains("stale")
    );
}

#[test]
fn saved_plan_xcode_partial_export_failure_preserves_previous_install() {
    let e = Env::new();
    e.write_manifest("[xcode]\nskills = ['a', 'b']\nos = ['macos', 'linux']\n");
    e.export_xcode(&[("a", "old a"), ("b", "old b")], "Xcode 27");
    assert!(e.xcode(&["sync"]).status.success());
    let lock = fs::read(e.config("skills.lock")).unwrap();
    e.export_xcode(&[("a", "new a")], "Xcode 28");
    assert!(
        !e.xcode(&["update", "--plan", "review.toml"])
            .status
            .success()
    );
    assert!(!e.home.join("review.toml").exists());
    assert_eq!(lock, fs::read(e.config("skills.lock")).unwrap());
    assert!(body(&e.codex("a")).contains("old a"));
    assert!(body(&e.codex("b")).contains("old b"));
}

#[test]
fn saved_plan_xcode_dry_apply_previews_replacement_and_detects_conflicts() {
    let e = Env::new();
    e.write_manifest("[xcode]\nskills = '*'\nos = ['macos', 'linux']\n");
    e.export_xcode(&[("a", "old a")], "Xcode 27");
    assert!(e.xcode(&["sync"]).status.success());
    e.export_xcode(&[("a", "new a"), ("b", "new b")], "Xcode 28");
    assert!(
        e.xcode(&["update", "--plan", "review.toml"])
            .status
            .success()
    );
    let lock = fs::read(e.config("skills.lock")).unwrap();
    let state = fs::read(e.home.join(".local/share/skl/state.toml")).unwrap();
    let output = e.ok(&["update", "--apply-plan", "review.toml", "--dry-run"]);
    assert!(output.contains("↑ a"), "{output}");
    assert!(!output.contains("removed"), "{output}");
    assert!(output.contains("2 links repointed"), "{output}");
    assert_eq!(
        fs::read_dir(e.home.join(".local/share/skl/store/commands"))
            .unwrap()
            .count(),
        1
    );
    assert!(body(&e.codex("a")).contains("old a"));
    assert!(!e.codex("b").exists());
    assert_eq!(fs::read(e.config("skills.lock")).unwrap(), lock);
    assert_eq!(
        fs::read(e.home.join(".local/share/skl/state.toml")).unwrap(),
        state
    );
    fs::create_dir(e.codex("b")).unwrap();
    fs::write(e.codex("b").join("user-file"), "keep").unwrap();
    let error = e.fails(&["update", "--apply-plan", "review.toml", "--dry-run"]);
    assert!(error.contains("skl didn't create it"), "{error}");
    assert_eq!(
        fs::read_to_string(e.codex("b").join("user-file")).unwrap(),
        "keep"
    );
    assert_eq!(fs::read(e.config("skills.lock")).unwrap(), lock);
    assert_eq!(
        fs::read(e.home.join(".local/share/skl/state.toml")).unwrap(),
        state
    );
}

#[test]
fn saved_plan_wildcard_exclusions_preserve_complete_coverage() {
    let e = Env::new();
    let url = e.repo("excludedplan", &[("a", "a"), ("b", "b")]);
    e.write_manifest(&format!(
        "[repositories.team]\nsource = '{url}'\nskills = '*'\nexclude = ['a']\n"
    ));
    e.ok(&["update", "--plan", "review.toml"]);
    let plan = fs::read_to_string(e.home.join("review.toml")).unwrap();
    assert!(plan.contains("a = \"a\""));
    write_skill(
        &e.remote("excludedplan").join("later"),
        "later",
        "unreviewed",
    );
    commit(&e.remote("excludedplan"), "later member");
    e.ok(&["update", "--apply-plan", "review.toml"]);
    assert!(!e.codex("a").exists());
    assert!(e.codex("b").exists());
    assert!(!e.codex("later").exists());
    let lock: toml::Value =
        toml::from_str(&fs::read_to_string(e.config("skills.lock")).unwrap()).unwrap();
    assert!(lock["sources"]["team"]["members"].get("a").is_some());
    assert!(lock["skills"].get("a").is_none());
}

fn fake_app() -> &'static Path {
    static BIN: std::sync::OnceLock<TempDir> = std::sync::OnceLock::new();
    BIN.get_or_init(|| {
        let dir = tempfile::tempdir().unwrap();
        let output = Command::new("rustc")
            .arg(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/fixtures/app.rs"
            ))
            .args(["--edition", "2021", "-o"])
            .arg(dir.path().join("skl-test-app"))
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
    fn app_setup(&self, extra: &str, directory: bool) {
        fs::write(
            self.remotes.join("content"),
            "# App skill
version one
",
        )
        .unwrap();
        let export = if directory { ", '{output}'" } else { "" };
        self.write_manifest(&format!(
            "[skills]
app = {{ command = ['skl-test-app', '{}'{export}], version-command = ['skl-test-app', '{}', '--version'], {extra} }}
",
            self.remotes.display(), self.remotes.display()
        ));
    }

    fn app(&self, args: &[&str]) -> Output {
        let path = std::env::join_paths(
            std::iter::once(fake_app().to_path_buf())
                .chain(std::env::split_paths(&std::env::var_os("PATH").unwrap())),
        )
        .unwrap();
        self.skl_with_env(
            args,
            &[("PATH", &path), ("SKL_TEST_SECRET", "secret".as_ref())],
        )
    }

    fn app_ok(&self, args: &[&str]) -> String {
        let out = self.app(args);
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8(out.stdout).unwrap()
    }
}

#[test]
fn command_snapshots_refresh_report_provenance_and_do_not_churn() {
    for directory in [false, true] {
        let env = Env::new();
        env.app_setup("", directory);
        env.app_ok(&["sync"]);
        let lock = fs::read(env.config("skills.lock")).unwrap();
        let target = fs::read_link(env.codex("app")).unwrap();
        assert!(body(&target).contains("version one"));
        assert!(String::from_utf8_lossy(&lock).contains("app 1.2.3"));
        assert!(target.to_string_lossy().contains("/store/commands/"));
        env.app_ok(&["sync"]);
        assert_eq!(lock, fs::read(env.config("skills.lock")).unwrap());
        fs::write(
            env.remotes.join("content"),
            "# App skill
version two
",
        )
        .unwrap();
        let diff = env.app_ok(&["update", "--patch"]);
        assert!(diff.contains("-version one"));
        assert!(diff.contains("+version two"));
        assert!(body(&env.codex("app")).contains("version two"));
        env.app_ok(&["verify", "app"]);
        fs::write(
            env.remotes.join("content"),
            "# App skill
version three
",
        )
        .unwrap();
        let diff = env.app_ok(&["sync"]);
        assert!(diff.contains("SKILL.md"));
        assert!(diff.contains("insertion"));
        assert!(!diff.contains("+version three"));
    }
}

#[test]
fn command_missing_unlinks_owned_links_preserves_lock_and_user_links() {
    let env = Env::new();
    env.app_setup("", false);
    env.app_ok(&["sync"]);
    let lock = fs::read(env.config("skills.lock")).unwrap();
    let target = fs::read_link(env.codex("app")).unwrap();
    fs::remove_file(env.claude("app")).unwrap();
    let user = env.home.join("manual");
    write_skill(&user, "app", "manual");
    std::os::unix::fs::symlink(&user, env.claude("app")).unwrap();
    let empty_path = env.home.join("empty-bin");
    fs::create_dir(&empty_path).unwrap();
    let out = env.skl_with_env(&["sync"], &[("PATH", empty_path.as_os_str())]);
    assert!(out.status.success());
    assert!(String::from_utf8_lossy(&out.stdout).contains("unavailable"));
    assert!(!env.codex("app").exists());
    assert_eq!(fs::read_link(env.claude("app")).unwrap(), user);
    assert_eq!(lock, fs::read(env.config("skills.lock")).unwrap());
    assert!(target.exists());
    fs::remove_file(env.claude("app")).unwrap();
    // Locked uses the cache even without the executable.
    assert!(
        env.skl_with_env(&["sync", "--locked"], &[("PATH", empty_path.as_os_str())])
            .status
            .success()
    );
    assert_eq!(fs::read_link(env.codex("app")).unwrap(), target);
}

#[test]
fn command_failures_preserve_previous_install() {
    for (mode, directory) in [
        ("fail", false),
        ("version-fail", false),
        ("descendant", false),
        ("stderr", false),
        ("timeout", false),
        ("empty", false),
        ("invalid", false),
        ("symlink", true),
        ("symlink-root", true),
        ("escape", true),
        ("fifo", true),
        ("large-tree", true),
        ("large", false),
    ] {
        let env = Env::new();
        env.app_setup(
            if matches!(mode, "timeout" | "descendant") {
                "timeout = 1"
            } else {
                ""
            },
            directory,
        );
        env.app_ok(&["sync"]);
        let lock = fs::read(env.config("skills.lock")).unwrap();
        let target = fs::read_link(env.codex("app")).unwrap();
        fs::write(env.remotes.join("mode"), mode).unwrap();
        let out = env.app(&["update"]);
        assert!(!out.status.success(), "{mode} should fail");
        assert_eq!(lock, fs::read(env.config("skills.lock")).unwrap(), "{mode}");
        assert_eq!(target, fs::read_link(env.codex("app")).unwrap(), "{mode}");
        assert!(body(&target).contains("version one"));
    }
}

#[test]
fn command_locked_dry_run_read_only_and_configure_never_execute() {
    let env = Env::new();
    env.app_setup("", false);
    env.app_ok(&["sync", "--dry-run"]);
    env.app_ok(&["update", "--dry-run", "--patch"]);
    assert!(!env.remotes.join("ran").exists());
    assert!(!env.config("skills.lock").exists());
    assert!(!env.home.join(".local/share/skl").exists());
    assert!(!env.app(&["sync", "--locked"]).status.success());
    env.app_ok(&["sync"]);
    fs::remove_file(env.remotes.join("ran")).unwrap();
    let lock = fs::read(env.config("skills.lock")).unwrap();
    for args in [
        vec!["sync", "--locked"],
        vec!["sync", "--dry-run"],
        vec!["update", "--dry-run", "--patch"],
        vec!["list"],
        vec!["list", "--json"],
        vec!["info", "app"],
        vec!["info", "app", "--json"],
        vec!["doctor"],
        vec!["verify"],
        vec!["configure", "app", "--agent", "codex"],
    ] {
        env.app_ok(&args);
        assert!(!env.remotes.join("ran").exists(), "{args:?}");
        assert_eq!(lock, fs::read(env.config("skills.lock")).unwrap());
    }
}

#[test]
fn command_profile_os_gates_and_optional_paths() {
    let env = Env::new();
    env.app_setup("profiles = ['personal']", false);
    env.app_ok(&["sync"]);
    assert!(!env.remotes.join("ran").exists());
    env.app_ok(&["--profile", "personal", "sync"]);
    let lock = fs::read(env.config("skills.lock")).unwrap();
    fs::remove_file(env.remotes.join("ran")).unwrap();
    env.app_ok(&["sync"]);
    assert!(!env.codex("app").exists());
    assert_eq!(lock, fs::read(env.config("skills.lock")).unwrap());
    let other = if cfg!(target_os = "macos") {
        "linux"
    } else {
        "macos"
    };
    env.app_setup(&format!("os = ['{other}']"), false);
    env.app_ok(&["sync"]);
    assert!(!env.remotes.join("ran").exists());

    env.write_manifest(
        "[skills]
app = { path = '~/bundled', optional = true }",
    );
    env.ok(&["sync"]);
    write_skill(&env.home.join("bundled"), "app", "bundled");
    env.ok(&["sync"]);
    assert!(env.codex("app").exists());
    fs::remove_dir_all(env.home.join("bundled")).unwrap();
    env.ok(&["sync"]);
    assert!(fs::symlink_metadata(env.codex("app")).is_err());
    // An installed app with a malformed bundle is an error, not absence.
    fs::create_dir(env.home.join("bundled")).unwrap();
    env.fails(&["sync"]);
}

#[test]
fn command_manifest_validation_and_locked_drift() {
    let env = Env::new();
    for entry in [
        "command = []",
        "command = 'echo hi'",
        "command = ['echo'], path = '.'",
        "command = ['echo'], source = 'a/b'",
        "command = ['echo'], ref = 'main'",
        "command = ['echo'], timeout = 0",
        "command = ['echo'], optional = true",
        "command = ['echo', '--out={output}']",
        "command = ['./echo']",
        "path = '.', version-command = ['echo']",
    ] {
        env.write_manifest(&format!(
            "[skills]
app = {{ {entry} }}"
        ));
        env.fails(&["list"]);
    }
    env.app_setup("", false);
    env.app_ok(&["sync"]);
    env.app_setup("timeout = 1", false);
    fs::remove_file(env.remotes.join("ran")).unwrap();
    assert!(!env.app(&["sync", "--locked"]).status.success());
    assert!(!env.remotes.join("ran").exists());
    assert!(!env.app(&["update", "--plan", "plan.json"]).status.success());
}

#[test]
fn command_initial_failure_and_missing_app_never_publish_content() {
    let env = Env::new();
    env.app_setup("", false);
    fs::write(env.remotes.join("mode"), "fail").unwrap();
    assert!(!env.app(&["sync"]).status.success());
    assert!(!env.codex("app").exists());
    let lock = fs::read_to_string(env.config("skills.lock")).unwrap();
    assert!(!lock.contains("commands.app"));
    assert!(env.skl(&["sync"]).status.success());
    assert!(!env.codex("app").exists());
}

#[test]
fn command_permission_errors_and_missing_version_probe_preserve_snapshot() {
    use std::os::unix::fs::PermissionsExt;
    let env = Env::new();
    let bin = env.home.join("bin");
    fs::create_dir(&bin).unwrap();
    let executable = bin.join("app");
    fs::copy(fake_app().join("skl-test-app"), &executable).unwrap();
    fs::write(env.remotes.join("content"), "# skill\n").unwrap();
    env.write_manifest(&format!(
        "[skills]\napp = {{ command = ['{}', '{}'] }}",
        executable.display(),
        env.remotes.display()
    ));
    env.ok(&["sync"]);
    let lock = fs::read(env.config("skills.lock")).unwrap();
    let target = fs::read_link(env.codex("app")).unwrap();
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o644)).unwrap();
    assert!(env.fails(&["sync"]).contains("not executable"));
    assert_eq!(lock, fs::read(env.config("skills.lock")).unwrap());
    assert_eq!(target, fs::read_link(env.codex("app")).unwrap());
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o755)).unwrap();
    env.write_manifest(&format!(
        "[skills]\napp = {{ command = ['{}', '{}'], version-command = ['skl-nonexistent-probe'] }}",
        executable.display(),
        env.remotes.display()
    ));
    assert!(env.fails(&["sync"]).contains("version-command"));
    assert_eq!(lock, fs::read(env.config("skills.lock")).unwrap());
    assert_eq!(target, fs::read_link(env.codex("app")).unwrap());
}

#[test]
fn command_normalization_and_targeted_update() {
    let env = Env::new();
    env.app_setup("", false);
    fs::write(
        env.remotes.join("content"),
        "---\nname: app\ndescription: Guide\n---\n# skill\n",
    )
    .unwrap();
    env.app_ok(&["sync"]);
    let lock = fs::read(env.config("skills.lock")).unwrap();
    fs::write(
        env.remotes.join("content"),
        "---\ndescription: Guide\nname: app\n---\n# skill\n",
    )
    .unwrap();
    env.app_ok(&["sync"]);
    assert_eq!(lock, fs::read(env.config("skills.lock")).unwrap());
    let mut manifest = fs::read_to_string(env.config("skills.toml")).unwrap();
    manifest.push_str("local = { path = '~/local' }\n");
    write_skill(&env.home.join("local"), "local", "local");
    env.write_manifest(&manifest);
    fs::remove_file(env.remotes.join("ran")).unwrap();
    env.app_ok(&["update", "local"]);
    assert!(!env.remotes.join("ran").exists());
    assert_eq!(lock, fs::read(env.config("skills.lock")).unwrap());
}

#[test]
fn app_source_exports_select_and_override_like_any_source() {
    let e = Env::new();
    let root = e.remotes.display().to_string();
    for name in ["alpha", "beta", "gamma"] {
        write_skill(&e.remotes.join("collection").join(name), name, "v1");
    }
    // A collection must name its output directory.
    e.write_manifest(&format!(
        "[sources.tools]\ncommand = ['skl-test-app', '{root}', '--collection']\nskills = '*'\n"
    ));
    assert!(e.fails(&["list"]).contains("{output}"));
    e.write_manifest(&format!(
        "[sources.tools]\ncommand = ['skl-test-app', '{root}', '--collection', '{{output}}']\nversion-command = ['skl-test-app', '{root}', '--version']\nskills = '*'\nexclude = ['gamma']\n\n[skills]\nbeta = {{ source = 'tools', agents = ['codex'] }}\n"
    ));
    let out = e.app_ok(&["sync"]);
    assert!(out.contains("tools app 1.2.3"), "{out}");
    assert!(e.claude("alpha").is_symlink() && e.codex("alpha").is_symlink());
    assert!(e.codex("beta").is_symlink() && !e.claude("beta").exists());
    assert!(!e.codex("gamma").exists());
    let lock: toml::Value =
        toml::from_str(&fs::read_to_string(e.config("skills.lock")).unwrap()).unwrap();
    assert_eq!(lock["version"].as_integer(), Some(3));
    // Members share the source's command and version, recording only a digest.
    assert!(lock.get("commands").is_none());
    let tools = &lock["sources"]["tools"];
    assert_eq!(tools["version"].as_str(), Some("app 1.2.3"));
    assert_eq!(tools["command"][0].as_str(), Some("skl-test-app"));
    assert_eq!(tools["snapshots"]["alpha"].as_str().map(str::len), Some(64));
    assert!(tools["members"].get("gamma").is_some());
    // Locked installs use the cache without running the app.
    fs::remove_file(e.remotes.join("ran")).unwrap();
    e.app_ok(&["sync", "--locked"]);
    assert!(!e.remotes.join("ran").exists());
    assert!(e.app_ok(&["list"]).contains("app: tools"));
    // Members are removed and configured like any source's, and the
    // automatic sync after an edit never runs the export.
    let out = e.app_ok(&["remove", "alpha"]);
    assert!(out.contains("tools: app export requires an explicit sync or update"));
    assert!(!e.remotes.join("ran").exists());
    assert!(!e.codex("alpha").exists());
    assert!(e.codex("beta").is_symlink());
    let manifest: toml::Value =
        toml::from_str(&fs::read_to_string(e.config("skills.toml")).unwrap()).unwrap();
    let excluded = manifest["sources"]["tools"]["exclude"].as_array().unwrap();
    assert!(excluded.iter().any(|v| v.as_str() == Some("alpha")));
    assert!(
        e.fails(&["configure", "gamma", "--agent", "codex", "--no-sync"])
            .contains("isn't in the manifest")
    );
    e.app_ok(&["sync"]);
    assert!(!e.codex("alpha").exists());
    // Plans never run arbitrary commands, so they reject command sources.
    assert!(
        e.fails(&["update", "--plan", "review.toml"])
            .contains("do not support command imports")
    );
}

#[test]
fn xcode_is_an_app_source_that_skips_when_xcode_is_missing() {
    let env = Env::new();
    env.write_manifest(
        "[sources.xcode]\napp = 'xcode'\nos = ['macos', 'linux']\nskills = ['alpha', 'beta']\n",
    );
    env.export_xcode(&[("alpha", "v1"), ("beta", "v1")], "Xcode 27");
    assert!(env.xcode(&["sync"]).status.success());
    assert!(env.codex("alpha").is_symlink());
    let lock = fs::read(env.config("skills.lock")).unwrap();
    // A Mac with only the Command Line Tools skips Xcode like any missing app.
    fs::write(env.remotes.join("missing"), "").unwrap();
    fs::remove_file(env.remotes.join("called")).unwrap();
    let out = env.xcode(&["sync"]);
    assert!(out.status.success());
    assert!(String::from_utf8_lossy(&out.stdout).contains("xcode: app unavailable; skipping"));
    assert!(!env.remotes.join("called").exists());
    assert!(!env.codex("alpha").exists());
    assert_eq!(lock, fs::read(env.config("skills.lock")).unwrap());
    // The cached export still installs with --locked, without asking Xcode.
    assert!(env.xcode(&["sync", "--locked"]).status.success());
    assert!(body(&env.codex("alpha")).contains("v1"));
    assert!(!env.remotes.join("called").exists());
    fs::remove_file(env.remotes.join("missing")).unwrap();

    // Configuring a member moves it out of the list into an override.
    env.ok(&["configure", "beta", "--agent", "codex", "--no-sync"]);
    let manifest: toml::Value =
        toml::from_str(&fs::read_to_string(env.config("skills.toml")).unwrap()).unwrap();
    assert_eq!(
        manifest["sources"]["xcode"]["skills"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(manifest["skills"]["beta"]["source"].as_str(), Some("xcode"));
    assert!(env.xcode(&["sync"]).status.success());
    assert!(env.codex("beta").is_symlink());
    assert!(!env.claude("beta").exists());
    // Only placement can be overridden for an app's skill.
    env.write_manifest(
        "[sources.xcode]\napp = 'xcode'\nskills = '*'\n[skills]\nbeta = { source = 'xcode', ref = 'main' }\n",
    );
    assert!(
        env.fails(&["list"])
            .contains("can only override agents, profiles, and os")
    );
    env.write_manifest("[xcode]\nskills = '*'\n[sources.xcode]\napp = 'xcode'\nskills = '*'\n");
    assert!(env.fails(&["list"]).contains("declare Xcode once"));
    env.write_manifest(
        "[sources.xcode]\napp = 'xcode'\ncommand = ['x', '{output}']\nskills = '*'\n",
    );
    assert!(env.fails(&["list"]).contains("either `app` or `command`"));
}

#[test]
fn version_two_xcode_locks_keep_working_and_migrate_on_the_next_export() {
    let env = Env::new();
    env.write_manifest("[xcode]\nos = ['macos', 'linux']\nskills = '*'\n");
    env.export_xcode(&[("alpha", "v1")], "Xcode 27");
    assert!(env.xcode(&["sync"]).status.success());
    // Rebuild what version 2 wrote: an [xcode] lock table and store/xcode content.
    let lock: toml::Value =
        toml::from_str(&fs::read_to_string(env.config("skills.lock")).unwrap()).unwrap();
    let digest = lock["sources"]["xcode"]["snapshots"]["alpha"]
        .as_str()
        .unwrap();
    let store = env.home.join(".local/share/skl/store");
    let legacy = store.join("xcode").join(digest).join("alpha");
    fs::create_dir_all(legacy.parent().unwrap()).unwrap();
    fs::rename(store.join("commands").join(digest).join("alpha"), &legacy).unwrap();
    for link in [env.claude("alpha"), env.codex("alpha")] {
        fs::remove_file(&link).unwrap();
        std::os::unix::fs::symlink(&legacy, &link).unwrap();
    }
    let state = env.home.join(".local/share/skl/state.toml");
    let text = fs::read_to_string(&state).unwrap();
    fs::write(&state, text.replace("store/commands/", "store/xcode/")).unwrap();
    let old = format!(
        "version = 2\nxcode_all_complete = true\n\n[xcode.alpha]\nversion = \"Xcode 27\"\ndigest = \"{digest}\"\n"
    );
    fs::write(env.config("skills.lock"), &old).unwrap();

    // Locked installs read the old lock and cache without rewriting either.
    fs::remove_file(env.remotes.join("called")).unwrap();
    assert!(env.xcode(&["sync", "--locked"]).status.success());
    assert!(!env.remotes.join("called").exists());
    assert_eq!(fs::read_to_string(env.config("skills.lock")).unwrap(), old);
    assert_eq!(fs::read_link(env.codex("alpha")).unwrap(), legacy);
    assert!(env.ok(&["list"]).contains("app: xcode"));
    env.ok(&["verify"]);

    // The next export moves the snapshot under commands and upgrades the lock.
    assert!(env.xcode(&["sync"]).status.success());
    let text = fs::read_to_string(env.config("skills.lock")).unwrap();
    let lock: toml::Value = toml::from_str(&text).unwrap();
    assert_eq!(lock["version"].as_integer(), Some(3));
    let xcode = &lock["sources"]["xcode"];
    assert_eq!(xcode["snapshots"]["alpha"].as_str(), Some(digest));
    assert_eq!(xcode["version"].as_str(), Some("Xcode 27"));
    assert!(xcode["members"].get("alpha").is_some());
    assert!(lock.get("xcode").is_none() && lock.get("commands").is_none());
    let target = fs::read_link(env.codex("alpha")).unwrap();
    assert!(target.starts_with(store.join("commands")));
    assert!(body(&env.codex("alpha")).contains("v1"));
    assert!(!store.join("xcode").join(digest).exists());
    assert!(env.xcode(&["sync", "--locked"]).status.success());
    // `remove` excludes a wildcard member, even under the [xcode] shorthand.
    env.ok(&["remove", "alpha"]);
    assert!(
        fs::read_to_string(env.config("skills.toml"))
            .unwrap()
            .contains("exclude = [\"alpha\"]")
    );
    assert!(!env.codex("alpha").exists());
}
