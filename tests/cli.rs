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
                .env("HOME", &env.home)
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
