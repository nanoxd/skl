use std::env;
use std::fmt;
use std::fs;
use std::io::ErrorKind;

use anyhow::{Context, Result};

use crate::manifest;
use crate::paths::Paths;

/// The profiles this run installs, such as `personal` on a laptop and nothing on a
/// server. Skills without `profiles` in the manifest install everywhere.
pub struct Active {
    pub names: Vec<String>,
    pub from: From,
}

pub enum From {
    Flag,
    Env,
    Machine,
    Unset,
}

impl fmt::Display for From {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            From::Flag => "--profile",
            From::Env => "SKL_PROFILE",
            From::Machine => "this machine's setting",
            From::Unset => "nothing set",
        })
    }
}

impl Active {
    /// `--profile` wins over `SKL_PROFILE`, which wins over the machine's own setting.
    pub fn resolve(paths: &Paths, flags: &[String]) -> Result<Self> {
        if !flags.is_empty() {
            return Self::new(flags.iter().cloned(), From::Flag);
        }
        if let Some(value) = env::var_os("SKL_PROFILE").filter(|v| !v.is_empty()) {
            let value = value.to_string_lossy().into_owned();
            return Self::new(value.split(',').map(str::to_string), From::Env);
        }
        match fs::read_to_string(paths.profile()) {
            Ok(text) => Self::new(text.lines().map(str::to_string), From::Machine),
            Err(err) if err.kind() == ErrorKind::NotFound => Ok(Self {
                names: Vec::new(),
                from: From::Unset,
            }),
            Err(err) => {
                Err(err).with_context(|| format!("reading {}", paths.tilde(&paths.profile())))
            }
        }
    }

    fn new(names: impl Iterator<Item = String>, from: From) -> Result<Self> {
        let mut active: Vec<String> = Vec::new();
        for name in names
            .map(|n| n.trim().to_string())
            .filter(|n| !n.is_empty())
        {
            validate(&name)?;
            if !active.contains(&name) {
                active.push(name);
            }
        }
        Ok(Self {
            names: active,
            from,
        })
    }

    pub fn includes(&self, profiles: &[String]) -> bool {
        profiles.is_empty() || profiles.iter().any(|p| self.names.contains(p))
    }
}

/// Saves the machine's profiles in the data directory, which stays out of dotfiles
/// so each machine keeps its own.
pub fn set(paths: &Paths, names: &[String]) -> Result<()> {
    let active = Active::new(names.iter().cloned(), From::Machine)?;
    crate::write_atomic(
        &paths.profile(),
        format!("{}\n", active.names.join("\n")).as_bytes(),
    )
}

pub fn clear(paths: &Paths) -> Result<()> {
    match fs::remove_file(paths.profile()) {
        Err(err) if err.kind() != ErrorKind::NotFound => Err(err.into()),
        _ => Ok(()),
    }
}

pub fn validate(name: &str) -> Result<()> {
    manifest::validate_name(name).with_context(|| format!("invalid profile `{name}`"))
}
