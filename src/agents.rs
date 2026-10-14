use std::path::PathBuf;

use anyhow::{Result, bail};

use crate::paths::Paths;

/// Agents skl knows by name, with the global directory each one reads skills from.
/// Anything else in the manifest's `agents` list must be a path.
pub const KNOWN: &[(&str, &str)] = &[
    ("claude-code", "~/.claude/skills"),
    ("codex", "~/.agents/skills"),
];

pub const DEFAULT: &[&str] = &["claude-code", "codex"];

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Agent {
    pub id: String,
    pub dir: PathBuf,
}

pub fn resolve(id: &str, paths: &Paths) -> Result<Agent> {
    if let Some((_, dir)) = KNOWN.iter().find(|(known, _)| *known == id) {
        return Ok(Agent {
            id: id.into(),
            dir: paths.expand(dir),
        });
    }
    if id.starts_with('~') || id.starts_with('/') {
        return Ok(Agent {
            id: id.into(),
            dir: paths.expand(id),
        });
    }
    let known: Vec<_> = KNOWN.iter().map(|(name, _)| *name).collect();
    bail!(
        "unknown agent `{id}`: use one of {} or a path starting with ~ or /",
        known.join(", ")
    )
}
