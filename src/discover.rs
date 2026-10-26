use anyhow::Result;

use crate::{git::Git, manifest};

/// Inspect a remote using disposable storage, independent of installed state.
pub fn run(source: &str, git_ref: Option<&str>) -> Result<bool> {
    let source = crate::source::Source::parse(source, git_ref, None)?;
    let url = manifest::source_url(&source.repository)?;
    let temporary = tempfile::tempdir()?;
    let git = Git::new(temporary.path().join("repos"));
    let commit = source.resolve(&git, &url)?;
    let repo = git.ensure_commit(&url, &commit)?;
    // A tree URL can name one skill or a collection folder holding several.
    let (mut candidates, _) = git.lookup(&repo, &commit, source.subdir.as_deref())?;
    if let Some(skill) = &source.skill {
        candidates.retain(|c| &c.name == skill || &c.dir_name == skill);
    }
    candidates.sort_by(|a, b| a.subdir.cmp(&b.subdir));
    println!("Source: {}", display(&url));
    println!("Commit: {commit}");
    let dirs: Vec<&str> = candidates.iter().map(|c| c.subdir.as_str()).collect();
    let files = git.files(&repo, &commit)?;
    for plugin in crate::plugin::containing(&git, &repo, &commit, &files, &dirs) {
        let root = if plugin.root.is_empty() {
            "."
        } else {
            &plugin.root
        };
        let extras = if plugin.has_more() {
            format!(", also ships {} that skl doesn't install", plugin.extras())
        } else {
            String::new()
        };
        println!(
            "Plugin: {} ({}){}",
            display(&plugin.name),
            display(root),
            display(&extras)
        );
    }
    if candidates.is_empty() {
        println!("No skills found (no regular-file SKILL.md).");
    } else {
        println!("\nNAME\tSUBDIRECTORY\tDESCRIPTION");
        for candidate in candidates {
            let subdir = if candidate.subdir.is_empty() {
                "."
            } else {
                &candidate.subdir
            };
            println!(
                "{}\t{}\t{}",
                display(&candidate.name),
                display(subdir),
                display(candidate.description.as_deref().unwrap_or(""))
            );
        }
    }
    Ok(true)
}

// Repository metadata must not introduce terminal controls or additional rows.
fn display(text: &str) -> String {
    text.chars()
        .flat_map(|c| {
            if c.is_control() {
                c.escape_default().collect::<Vec<_>>()
            } else {
                vec![c]
            }
        })
        .collect()
}
