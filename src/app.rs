//! Manifest commands are trusted programs; resource limits are not a sandbox.
use std::fs;
use std::io::{ErrorKind, Read};
use std::os::fd::AsRawFd;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use crate::{
    lock::{CommandSkill, XcodeSkill},
    manifest::CommandOrigin,
    paths::Paths,
    xcode::Snapshot,
};
use anyhow::{Context, Result, bail};

const PIPE_LIMIT: usize = 8 * 1024 * 1024;
const TREE_LIMIT: u64 = 32 * 1024 * 1024;

pub fn skill_dir(paths: &Paths, name: &str, entry: &CommandSkill) -> PathBuf {
    paths
        .store()
        .join("commands")
        .join(&entry.snapshot.digest)
        .join(name)
}

fn search_path() -> std::ffi::OsString {
    std::env::join_paths(
        std::env::split_paths(&std::env::var_os("PATH").unwrap_or_else(|| "/usr/bin:/bin".into()))
            .filter(|p| p.is_absolute()),
    )
    .unwrap_or_default()
}

pub fn executable(name: &str) -> Result<Option<PathBuf>> {
    use std::os::unix::fs::PermissionsExt;
    let candidates = if Path::new(name).is_absolute() {
        vec![PathBuf::from(name)]
    } else {
        std::env::split_paths(&search_path())
            .map(|p| p.join(name))
            .collect()
    };
    let mut unavailable = None;
    for path in candidates {
        match fs::metadata(&path) {
            Ok(meta) if meta.is_file() && meta.permissions().mode() & 0o111 != 0 => {
                return Ok(Some(path));
            }
            Ok(_) => unavailable = Some(format!("{} is not executable", path.display())),
            Err(err) if err.kind() == ErrorKind::NotFound => {}
            Err(err) => unavailable = Some(format!("{}: {err}", path.display())),
        }
    }
    if let Some(message) = unavailable {
        bail!(message);
    }
    Ok(None)
}

struct Process(std::process::Child);
impl Drop for Process {
    fn drop(&mut self) {
        // Children inherit this dedicated group; also close descendant-held pipes.
        unsafe {
            libc::kill(-(self.0.id() as i32), libc::SIGKILL);
        }
        let _ = self.0.wait();
    }
}

fn nonblocking(pipe: &impl AsRawFd) -> Result<()> {
    let fd = pipe.as_raw_fd();
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags < 0 || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    Ok(())
}

fn drain(pipe: &mut impl Read, bytes: &mut Vec<u8>) -> Result<bool> {
    let mut buf = [0; 8192];
    for _ in 0..64 {
        match pipe.read(&mut buf) {
            Ok(0) => return Ok(true),
            Ok(n) => {
                if bytes.len() + n > PIPE_LIMIT {
                    bail!("command output exceeds 8 MiB");
                }
                bytes.extend_from_slice(&buf[..n]);
            }
            Err(e) if e.kind() == ErrorKind::WouldBlock => return Ok(false),
            Err(e) if e.kind() == ErrorKind::Interrupted => continue,
            Err(e) => return Err(e.into()),
        }
    }
    Ok(false)
}

fn tree_limit(root: &Path) -> Result<()> {
    fn visit(dir: &Path, size: &mut u64, count: &mut usize, depth: usize) -> Result<()> {
        if depth > 64 {
            bail!("command export is too deeply nested");
        }
        for entry in fs::read_dir(dir)? {
            let path = entry?.path();
            let meta = fs::symlink_metadata(&path)?;
            *count += 1;
            *size += meta.len();
            if *count > 4096 || *size > TREE_LIMIT {
                bail!("command export exceeds 32 MiB or 4096 entries");
            }
            if meta.is_dir() {
                visit(&path, size, count, depth + 1)?;
            } else if !meta.is_file() {
                bail!("command export contains a symlink or special file");
            }
        }
        Ok(())
    }
    if root.exists() {
        visit(root, &mut 0, &mut 0, 0)?;
    }
    Ok(())
}

fn run(argv: &[String], program: &Path, cwd: &Path, dest: &Path, timeout: u64) -> Result<Vec<u8>> {
    let mut command = Command::new(program);
    for arg in &argv[1..] {
        if arg == "{output}" {
            command.arg(dest);
        } else {
            command.arg(arg);
        }
    }
    let child = command
        .current_dir(cwd)
        .env_clear()
        .env("PATH", search_path())
        .env("HOME", cwd)
        .env("TMPDIR", cwd)
        .env("LANG", "C")
        .env("LC_ALL", "C")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0)
        .spawn()
        .with_context(|| format!("running {}", argv[0]))?;
    let mut process = Process(child);
    let mut stdout = process.0.stdout.take().unwrap();
    let mut stderr = process.0.stderr.take().unwrap();
    nonblocking(&stdout)?;
    nonblocking(&stderr)?;
    let mut output = Vec::new();
    let mut errors = Vec::new();
    let start = Instant::now();
    loop {
        if start.elapsed() >= Duration::from_secs(timeout) {
            bail!("command timed out after {timeout}s");
        }
        let out_done = drain(&mut stdout, &mut output)?;
        let err_done = drain(&mut stderr, &mut errors)?;
        tree_limit(cwd)?;
        if let Some(status) = process.0.try_wait()? {
            if !status.success() {
                bail!(
                    "command exited with {status}: {}",
                    String::from_utf8_lossy(&errors).trim()
                );
            }
            if out_done && err_done {
                return Ok(output);
            }
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// None means the producer is not installed. All execution/validation failures
/// are errors, including a missing optional version probe.
pub fn import(origin: &CommandOrigin) -> Result<Option<(CommandSkill, Snapshot)>> {
    let Some(program) = executable(&origin.command[0])? else {
        return Ok(None);
    };
    let stage = tempfile::tempdir()?;
    let dest = stage.path().join("output");
    let output = run(
        &origin.command,
        &program,
        stage.path(),
        &dest,
        origin.timeout,
    )?;
    if !origin.command.iter().any(|a| a == "{output}") {
        fs::create_dir(&dest)?;
        let text = String::from_utf8(output).context("skill output is not UTF-8")?;
        fs::write(dest.join("SKILL.md"), text)?;
    }
    if !fs::symlink_metadata(&dest)?.is_dir()
        || !fs::symlink_metadata(dest.join("SKILL.md"))?.is_file()
    {
        bail!("command must produce a directory with a regular-file SKILL.md");
    }
    tree_limit(&dest)?;
    let skill = dest.join("SKILL.md");
    let text = fs::read_to_string(&skill)?;
    if text.trim().is_empty() || text.contains('\0') {
        bail!("SKILL.md must contain nonempty text without NUL bytes");
    }
    let normalized = crate::xcode::normalize_frontmatter(&text);
    if text != normalized {
        fs::write(&skill, normalized)?;
    }
    let snapshot = Snapshot::capture(&dest)?;
    let digest = crate::xcode::content_digest(&dest)?;
    let version = if let Some(argv) = &origin.version_command {
        let program = executable(&argv[0])?.context("version-command executable is unavailable")?;
        let probe = tempfile::tempdir()?;
        String::from_utf8(run(
            argv,
            &program,
            probe.path(),
            &probe.path().join("output"),
            origin.timeout,
        )?)?
        .trim()
        .to_owned()
    } else {
        String::new()
    };
    Ok(Some((
        CommandSkill {
            producer: origin.clone(),
            snapshot: XcodeSkill { version, digest },
        },
        snapshot,
    )))
}
