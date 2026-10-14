use std::env;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

/// Every location skl reads or writes, resolved once from the environment.
pub struct Paths {
    pub home: PathBuf,
    /// HOME with symlinks resolved, so paths from the working directory still shorten.
    home_real: Option<PathBuf>,
    pub config: PathBuf,
    pub data: PathBuf,
}

impl Paths {
    pub fn from_env() -> Result<Self> {
        let home = env::var_os("HOME")
            .filter(|h| !h.is_empty())
            .map(PathBuf::from)
            .context("HOME is not set")?;
        let config = dir_from_env("SKL_CONFIG_DIR", "XDG_CONFIG_HOME")
            .unwrap_or_else(|| home.join(".config/skl"));
        let data = dir_from_env("SKL_DATA_DIR", "XDG_DATA_HOME")
            .unwrap_or_else(|| home.join(".local/share/skl"));
        let home_real = home.canonicalize().ok().filter(|real| *real != home);
        Ok(Self {
            home,
            home_real,
            config,
            data,
        })
    }

    pub fn manifest(&self) -> PathBuf {
        self.config.join("skills.toml")
    }

    pub fn lock(&self) -> PathBuf {
        self.config.join("skills.lock")
    }

    pub fn repos(&self) -> PathBuf {
        self.data.join("repos")
    }

    pub fn store(&self) -> PathBuf {
        self.data.join("store")
    }

    pub fn state(&self) -> PathBuf {
        self.data.join("state.toml")
    }

    /// This machine's active profiles. Lives in the data directory so dotfiles never
    /// carry it to another machine.
    pub fn profile(&self) -> PathBuf {
        self.data.join("profile")
    }

    pub fn backups(&self) -> PathBuf {
        self.data.join("backup")
    }

    /// Expands `~`, and resolves relative paths against the manifest's directory so a
    /// dotfiles repo can keep local skills next to its manifest.
    pub fn expand(&self, path: &str) -> PathBuf {
        if path == "~" {
            self.home.clone()
        } else if let Some(rest) = path.strip_prefix("~/") {
            self.home.join(rest)
        } else {
            let path = Path::new(path);
            if path.is_absolute() {
                path.to_path_buf()
            } else {
                self.config.join(path)
            }
        }
    }

    /// Shortens paths under HOME to `~/...` for output and for portable manifest entries.
    pub fn tilde(&self, path: &Path) -> String {
        let rest = path.strip_prefix(&self.home).or_else(|err| {
            self.home_real
                .as_ref()
                .map_or(Err(err), |real| path.strip_prefix(real))
        });
        match rest {
            Ok(rest) if rest.as_os_str().is_empty() => "~".into(),
            Ok(rest) => format!("~/{}", rest.display()),
            Err(_) => path.display().to_string(),
        }
    }
}

fn dir_from_env(own: &str, xdg: &str) -> Option<PathBuf> {
    let non_empty = |key| {
        env::var_os(key)
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
    };
    non_empty(own).or_else(|| non_empty(xdg).map(|d| d.join("skl")))
}
