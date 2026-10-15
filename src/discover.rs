use anyhow::Result;

use crate::{git::Git, manifest};

/// Inspect a remote using disposable storage, independent of installed state.
pub fn run(source: &str, git_ref: Option<&str>) -> Result<bool> {
    let url = manifest::source_url(source)?;
    let temporary = tempfile::tempdir()?;
    let git = Git::new(temporary.path().join("repos"));
    let commit = git.resolve(&url, git_ref)?;
    let repo = git.ensure_commit(&url, &commit)?;
    let mut candidates = git.candidates(&repo, &commit)?;
    candidates.sort_by(|a, b| a.subdir.cmp(&b.subdir));
    println!("Source: {}", display(&url));
    println!("Commit: {commit}");
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
