//! Explicit, terminal-only selection before acquiring the write lock.
use std::io::{self, IsTerminal, Write};

use anyhow::{Context, Result, bail};

use crate::{SkillOptions, agents, git::Git, manifest, paths::Paths, source::Source};

/// Returns whether local names came from collection selection, rather than aliases
/// supplied for the single root skill. add preserves that distinction when revalidating.
pub fn select(
    paths: &Paths,
    source: &Source,
    skills: &mut Vec<String>,
    all: bool,
    options: &mut SkillOptions,
) -> Result<bool> {
    if !io::stdin().is_terminal() || !io::stderr().is_terminal() {
        bail!(
            "--interactive requires terminal stdin and stderr; use --skill, --all and --agent for scripts"
        );
    }
    options.validate(paths)?;
    if source.skill.is_some()
        && (all || (!skills.is_empty() && skills.first() != source.skill.as_ref()))
    {
        bail!("@skill conflicts with --all or --skill");
    }
    // Keep discovery outside the shared cache/run lock. add revalidates after locking.
    let local = if source.repository.starts_with('~') {
        paths.expand(&source.repository)
    } else {
        std::env::current_dir()?.join(&source.repository)
    };
    let mut local_collection = false;
    if local.is_dir() && (source.git_ref.is_some() || source.subdir.is_some()) {
        bail!("--ref and --subdir do not apply to local directories");
    }
    if skills.is_empty() && !all && source.skill.is_none() {
        let candidates = if local.is_dir() {
            local_collection = true;
            crate::git::local_candidates(&local)?
        } else {
            let temporary = tempfile::tempdir()?;
            let git = Git::new(temporary.path().join("repos"));
            let url = manifest::source_url(&source.repository)?;
            let commit = source.resolve(&git, &url)?;
            let repo = git.ensure_commit(&url, &commit)?;
            git.candidates_in(&repo, &commit, source.subdir.as_deref())?
        };
        if candidates.is_empty() {
            bail!("{} has no SKILL.md", source.repository);
        }
        let mut names = std::collections::BTreeSet::new();
        for candidate in &candidates {
            if !names.insert(&candidate.name) {
                bail!(
                    "ambiguous skill `{}`; add a specific local directory or use --subdir for git",
                    candidate.name
                );
            }
        }
        let labels: Vec<_> = candidates
            .iter()
            .map(|c| {
                format!(
                    "{} ({})",
                    c.name,
                    if c.subdir.is_empty() { "." } else { &c.subdir }
                )
            })
            .collect();
        *skills = prompt("Skills", &labels)?
            .into_iter()
            .map(|i| candidates[i].name.clone())
            .collect();
    }
    if options.agents.is_empty() {
        let detected = agents::detect(paths)?;
        let labels: Vec<_> = agents::known(paths)
            .iter()
            .map(|(id, _)| {
                if detected.iter().any(|agent| agent.id == *id) {
                    format!("{id} (configuration found)")
                } else {
                    (*id).into()
                }
            })
            .collect();
        options.agents = prompt("Agents", &labels)?
            .into_iter()
            .map(|i| agents::known(paths)[i].0.into())
            .collect();
    }
    Ok(local_collection)
}

fn prompt(title: &str, labels: &[String]) -> Result<Vec<usize>> {
    let mut stderr = io::stderr().lock();
    writeln!(stderr, "{title}:")?;
    for (index, label) in labels.iter().enumerate() {
        // Repository metadata must not inject control sequences into the prompt.
        let safe: String = label
            .chars()
            .flat_map(|c| {
                if c.is_control() {
                    c.escape_default().collect::<Vec<_>>()
                } else {
                    vec![c]
                }
            })
            .collect();
        writeln!(stderr, "  {}. {safe}", index + 1)?;
    }
    write!(
        stderr,
        "Choose numbers separated by commas, 'all', or 'q' to cancel: "
    )?;
    stderr.flush()?;
    let mut line = String::new();
    if io::stdin().read_line(&mut line)? == 0 {
        bail!("selection cancelled (end of input)");
    }
    parse_selection(&line, labels.len())
}

fn parse_selection(line: &str, count: usize) -> Result<Vec<usize>> {
    let line = line.trim();
    if line.is_empty() || line.eq_ignore_ascii_case("q") {
        bail!("selection cancelled");
    }
    if line == "all" {
        return Ok((0..count).collect());
    }
    let mut selected = Vec::new();
    for entry in line.split(',') {
        let number: usize = entry
            .trim()
            .parse()
            .context("invalid selection: enter comma-separated numbers")?;
        if number == 0 || number > count {
            bail!("invalid selection: choose a number from 1 to {count}");
        }
        if !selected.contains(&(number - 1)) {
            selected.push(number - 1);
        }
    }
    Ok(selected)
}

#[cfg(test)]
mod tests {
    use super::parse_selection;
    #[test]
    fn selections_are_explicit_and_bounded() {
        assert_eq!(parse_selection(" 2, 1,2\n", 3).unwrap(), [1, 0]);
        assert_eq!(parse_selection("all", 3).unwrap(), [0, 1, 2]);
        for bad in ["", "q", "0", "4", "1,", "1,wat", "-1"] {
            assert!(parse_selection(bad, 3).is_err(), "{bad}");
        }
    }
}
