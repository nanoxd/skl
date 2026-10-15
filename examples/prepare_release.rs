use anyhow::{Context, Result, bail};

/// Promote only the tagged snapshot's unreleased notes. Existing releases are
/// unchanged, so reruns and manually prepared release headings are supported.
pub fn prepare(changelog: &str, tag: &str, date: &str) -> Result<(String, String)> {
    let version = tag
        .strip_prefix('v')
        .context("release tag must start with v")?;
    if version.is_empty()
        || !version
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b".-+".contains(&c))
    {
        bail!("invalid release tag: {tag}");
    }
    let heading = format!("## [{version}]");
    let section = |name: &str| {
        let mut offset = 0;
        for line in changelog.split_inclusive('\n') {
            let start = offset;
            offset += line.len();
            let text = line.trim_end();
            if text == name || text.starts_with(&format!("{name} - ")) {
                let mut end = offset;
                for next in changelog[offset..].split_inclusive('\n') {
                    if next.starts_with("## ") {
                        break;
                    }
                    end += next.len();
                }
                return Some((start, offset, end));
            }
        }
        None
    };
    let existing = section(&heading);
    let (start, body, end) = existing
        .or_else(|| section("## [Unreleased]"))
        .context("CHANGELOG.md has neither a release entry nor [Unreleased]")?;
    let notes = changelog[body..end].trim();
    if !notes
        .lines()
        .any(|line| !line.trim().is_empty() && !line.starts_with('#'))
    {
        bail!("Missing or empty changelog entry for {version}");
    }
    let updated = if existing.is_some() {
        changelog.to_owned()
    } else {
        format!(
            "{}## [Unreleased]\n\n{heading} - {date}\n{}",
            &changelog[..start],
            &changelog[body..]
        )
    };
    Ok((updated, format!("{notes}\n")))
}

#[allow(dead_code)] // Also included by the CLI integration tests.
fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().collect();
    if args.len() != 3 {
        bail!("usage: prepare_release <tag> <YYYY-MM-DD>");
    }
    let original = std::fs::read_to_string("CHANGELOG.md").context("reading CHANGELOG.md")?;
    let (updated, notes) = prepare(&original, &args[1], &args[2])?;
    // These are disposable workflow checkout files; never write user config.
    std::fs::write("release-notes.md", notes).context("writing release-notes.md")?;
    if updated != original {
        std::fs::write("CHANGELOG.md", updated).context("writing CHANGELOG.md")?;
    }
    Ok(())
}
