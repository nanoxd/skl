use std::path::PathBuf;

use anyhow::{Result, bail};

use crate::paths::Paths;

/// Agents skl knows by name, with the global directory each one reads skills from.
/// Anything else in the manifest's `agents` list must be a path.
// Global paths cross-checked against vercel-labs/skills src/agents.ts.
// Project-only harnesses are intentionally excluded.
pub const KNOWN: &[(&str, &str)] = &[
    ("adal", "~/.adal/skills"),
    ("aider-desk", "~/.aider-desk/skills"),
    ("amp", "~/.config/agents/skills"),
    ("antigravity", "~/.gemini/antigravity/skills"),
    ("antigravity-cli", "~/.gemini/antigravity-cli/skills"),
    ("astrbot", "~/.astrbot/data/skills"),
    ("augment", "~/.augment/skills"),
    ("autohand-code", "~/.autohand/skills"),
    ("bob", "~/.bob/skills"),
    ("claude-code", "~/.claude/skills"),
    ("cline", "~/.agents/skills"),
    ("codearts-agent", "~/.codeartsdoer/skills"),
    ("codebuddy", "~/.codebuddy/skills"),
    ("codemaker", "~/.codemaker/skills"),
    ("codestudio", "~/.codestudio/skills"),
    ("codex", "~/.agents/skills"),
    ("command-code", "~/.commandcode/skills"),
    ("continue", "~/.continue/skills"),
    ("cortex", "~/.snowflake/cortex/skills"),
    ("crush", "~/.config/crush/skills"),
    ("cursor", "~/.cursor/skills"),
    ("deepagents", "~/.deepagents/agent/skills"),
    ("devin", "~/.config/devin/skills"),
    ("dexto", "~/.agents/skills"),
    ("droid", "~/.factory/skills"),
    ("firebender", "~/.firebender/skills"),
    ("forgecode", "~/.forge/skills"),
    ("fx", "~/.agents/skills"),
    ("gemini-cli", "~/.gemini/skills"),
    ("github-copilot", "~/.copilot/skills"),
    ("goose", "~/.config/goose/skills"),
    ("grok", "~/.grok/skills"),
    ("hermes-agent", "~/.hermes/skills"),
    ("iflow-cli", "~/.iflow/skills"),
    ("inference-sh", "~/.inferencesh/skills"),
    ("jazz", "~/.jazz/skills"),
    ("junie", "~/.junie/skills"),
    ("kilo", "~/.kilo/skills"),
    ("kimchi", "~/.config/kimchi/harness/skills"),
    ("kimi-code-cli", "~/.agents/skills"),
    ("kiro-cli", "~/.kiro/skills"),
    ("kode", "~/.kode/skills"),
    ("lingma", "~/.lingma/skills"),
    ("loaf", "~/.agents/skills"),
    ("mcpjam", "~/.mcpjam/skills"),
    ("minimax-code", "~/.minimax/skills"),
    ("mistral-vibe", "~/.vibe/skills"),
    ("moxby", "~/.moxby/skills"),
    ("mux", "~/.mux/skills"),
    ("neovate", "~/.neovate/skills"),
    ("ona", "~/.ona/skills"),
    ("openclaw", "~/.openclaw/skills"),
    ("opencode", "~/.config/opencode/skills"),
    ("openhands", "~/.openhands/skills"),
    ("pi", "~/.agents/skills"),
    ("pochi", "~/.pochi/skills"),
    ("posit-assistant", "~/.posit/assistant/skills"),
    ("qoder", "~/.qoder/skills"),
    ("qoder-cn", "~/.qoder-cn/skills"),
    ("qwen-code", "~/.qwen/skills"),
    ("reasonix", "~/.reasonix/skills"),
    ("replit", "~/.config/agents/skills"),
    ("roo", "~/.roo/skills"),
    ("rovodev", "~/.rovodev/skills"),
    ("sarvam-code", "~/.agents/skills"),
    ("tabnine-cli", "~/.tabnine/agent/skills"),
    ("terramind", "~/.terramind/skills"),
    ("tinycloud", "~/.tinycloud/skills"),
    ("trae", "~/.trae/skills"),
    ("trae-cn", "~/.trae-cn/skills"),
    ("universal", "~/.config/agents/skills"),
    ("warp", "~/.agents/skills"),
    ("windsurf", "~/.codeium/windsurf/skills"),
    ("zcode", "~/.zcode/skills"),
    ("zed", "~/.agents/skills"),
    ("zencoder", "~/.zencoder/skills"),
    ("zenflow", "~/.zencoder/skills"),
];

// Project paths cross-checked against vercel-labs/skills src/agents.ts.
pub const PROJECT: &[(&str, &str)] = &[
    ("adal", ".adal/skills"),
    ("aider-desk", ".aider-desk/skills"),
    ("amp", ".agents/skills"),
    ("antigravity", ".agents/skills"),
    ("antigravity-cli", ".agents/skills"),
    ("astrbot", "data/skills"),
    ("augment", ".augment/skills"),
    ("autohand-code", ".autohand/skills"),
    ("bob", ".bob/skills"),
    ("claude-code", ".claude/skills"),
    ("cline", ".agents/skills"),
    ("codearts-agent", ".codeartsdoer/skills"),
    ("codebuddy", ".codebuddy/skills"),
    ("codemaker", ".codemaker/skills"),
    ("codestudio", ".codestudio/skills"),
    ("codex", ".agents/skills"),
    ("command-code", ".commandcode/skills"),
    ("continue", ".continue/skills"),
    ("cortex", ".cortex/skills"),
    ("crush", ".crush/skills"),
    ("cursor", ".agents/skills"),
    ("deepagents", ".agents/skills"),
    ("devin", ".devin/skills"),
    ("dexto", ".agents/skills"),
    ("droid", ".agents/skills"),
    ("eve", "agent/skills"),
    ("firebender", ".agents/skills"),
    ("forgecode", ".forge/skills"),
    ("fx", ".agents/skills"),
    ("gemini-cli", ".agents/skills"),
    ("github-copilot", ".agents/skills"),
    ("goose", ".goose/skills"),
    ("grok", ".grok/skills"),
    ("hermes-agent", ".hermes/skills"),
    ("iflow-cli", ".iflow/skills"),
    ("inference-sh", ".inferencesh/skills"),
    ("jazz", ".jazz/skills"),
    ("junie", ".junie/skills"),
    ("kilo", ".agents/skills"),
    ("kimchi", ".kimchi/skills"),
    ("kimi-code-cli", ".agents/skills"),
    ("kiro-cli", ".kiro/skills"),
    ("kode", ".kode/skills"),
    ("lingma", ".lingma/skills"),
    ("loaf", ".agents/skills"),
    ("mcpjam", ".mcpjam/skills"),
    ("minimax-code", ".minimax/skills"),
    ("mistral-vibe", ".vibe/skills"),
    ("moxby", ".moxby/skills"),
    ("mux", ".mux/skills"),
    ("neovate", ".neovate/skills"),
    ("ona", ".ona/skills"),
    ("openclaw", "skills"),
    ("opencode", ".agents/skills"),
    ("openhands", ".openhands/skills"),
    ("pi", ".agents/skills"),
    ("pochi", ".pochi/skills"),
    ("posit-assistant", ".posit/assistant/skills"),
    ("promptscript", ".agents/skills"),
    ("qoder", ".qoder/skills"),
    ("qoder-cn", ".qoder/skills"),
    ("qwen-code", ".qwen/skills"),
    ("reasonix", ".reasonix/skills"),
    ("replit", ".agents/skills"),
    ("roo", ".roo/skills"),
    ("rovodev", ".rovodev/skills"),
    ("sarvam-code", ".agents/skills"),
    ("tabnine-cli", ".tabnine/agent/skills"),
    ("terramind", ".terramind/skills"),
    ("tinycloud", ".tinycloud/skills"),
    ("trae", ".trae/skills"),
    ("trae-cn", ".trae/skills"),
    ("universal", ".agents/skills"),
    ("warp", ".agents/skills"),
    ("windsurf", ".windsurf/skills"),
    ("zcode", ".zcode/skills"),
    ("zed", ".agents/skills"),
    ("zencoder", ".zencoder/skills"),
    ("zenflow", ".zencoder/skills"),
];

pub fn known(paths: &Paths) -> &'static [(&'static str, &'static str)] {
    if paths.project.is_some() {
        PROJECT
    } else {
        KNOWN
    }
}

pub const DEFAULT: &[&str] = &["claude-code", "codex"];

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Agent {
    pub id: String,
    pub dir: PathBuf,
}

pub fn resolve(id: &str, paths: &Paths) -> Result<Agent> {
    if let Some(root) = &paths.project
        && let Some((_, dir)) = PROJECT.iter().find(|(known, _)| *known == id)
    {
        return Ok(Agent {
            id: id.into(),
            dir: root.join(dir),
        });
    }
    if paths.project.is_none()
        && let Some((_, dir)) = KNOWN.iter().find(|(known, _)| *known == id)
    {
        return Ok(Agent {
            id: id.into(),
            dir: match id {
                "amp" => config_home(paths).join("agents/skills"),
                "devin" => config_home(paths).join("devin/skills"),
                "goose" => config_home(paths).join("goose/skills"),
                "opencode" => config_home(paths).join("opencode/skills"),
                "replit" => config_home(paths).join("agents/skills"),
                "universal" => config_home(paths).join("agents/skills"),
                _ => paths.expand(dir),
            },
        });
    }
    if id.starts_with('~')
        || id.starts_with('/')
        || (paths.project.is_some() && id.starts_with("./"))
    {
        return Ok(Agent {
            id: id.into(),
            dir: paths.expand(id),
        });
    }
    let known: Vec<_> = known(paths).iter().map(|(name, _)| *name).collect();
    bail!(
        "unknown agent `{id}`: use one of {} or a path starting with ~ or / (also ./ in project scope)",
        known.join(", ")
    )
}

fn config_home(paths: &Paths) -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .unwrap_or_else(|| paths.home.join(".config"))
}

/// Configuration directories are hints, not proof that a harness is installed.
/// Shared skill directories cannot identify the applications that consume them.
pub fn detect(paths: &Paths) -> Result<Vec<Agent>> {
    let mut detected = Vec::new();
    for (id, directory) in known(paths) {
        let agent = resolve(id, paths)?;
        let base = paths.project.as_ref().unwrap_or(&paths.home);
        let config = match *id {
            "codex" => base.join(".codex"),
            "cline" => base.join(".cline"),
            "pi" => base.join(".pi/agent"),
            "zed" if paths.project.is_none() => config_home(paths).join("zed"),
            "zed" => base.join(".zed"),
            "cursor" if paths.project.is_some() => base.join(".cursor"),
            "openclaw" if paths.project.is_some() => base.join("skills"),
            _ => {
                // A registry path shared by several harnesses supplies no identity.
                if known(paths)
                    .iter()
                    .filter(|(_, dir)| dir == directory)
                    .count()
                    > 1
                {
                    continue;
                }
                agent.dir.parent().unwrap().to_path_buf()
            }
        };
        if config.is_dir() {
            detected.push(agent);
        }
    }
    Ok(detected)
}
