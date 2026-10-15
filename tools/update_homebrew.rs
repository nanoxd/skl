use anyhow::{Context, Result, bail, ensure};
use sha2::{Digest, Sha256};
use std::path::Path;

const TARGETS: [&str; 4] = [
    "aarch64-apple-darwin",
    "x86_64-apple-darwin",
    "aarch64-unknown-linux-gnu",
    "x86_64-unknown-linux-gnu",
];
const BASE: &str = "https://github.com/nanoxd/skl/releases/download/";

fn version(tag: &str) -> Result<[u64; 3]> {
    let parts = tag
        .strip_prefix('v')
        .context("release tag must start with v")?
        .split('.')
        .map(|part| {
            ensure!(
                !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()),
                "expected stable vMAJOR.MINOR.PATCH tag: {tag}"
            );
            Ok(part.parse::<u64>()?)
        })
        .collect::<Result<Vec<_>>>()?;
    parts
        .try_into()
        .map_err(|_| anyhow::anyhow!("invalid release tag: {tag}"))
}

/// Update only the four download/checksum pairs, preserving the rest of the formula.
/// Verify all archives before returning any changes. Older releases never downgrade it.
pub fn update(formula: &str, tag: &str, dist: &Path) -> Result<String> {
    // The stable tap must not follow prerelease or build-metadata tags.
    if tag.contains(['-', '+']) {
        return Ok(formula.to_owned());
    }
    let next = version(tag)?;
    let mut lines: Vec<String> = formula.lines().map(str::to_owned).collect();
    let mut replacements = Vec::new();
    let mut current = None;
    for target in TARGETS {
        let archive = format!("skl-{target}.tar.gz");
        let suffix = format!("/{archive}\"");
        let matches: Vec<_> = lines
            .iter()
            .enumerate()
            .filter(|(_, line)| {
                line.trim_start().starts_with(&format!("url \"{BASE}")) && line.ends_with(&suffix)
            })
            .collect();
        ensure!(matches.len() == 1, "expected one formula URL for {target}");
        let (index, line) = matches[0];
        let old_tag = line
            .trim()
            .strip_prefix(&format!("url \"{BASE}"))
            .unwrap()
            .strip_suffix(&suffix)
            .unwrap();
        let old = version(old_tag)?;
        if let Some(current) = current {
            ensure!(old == current, "formula has inconsistent release versions");
        }
        current = Some(old);
        let checksum_line = lines.get(index + 1).context("missing formula checksum")?;
        ensure!(
            checksum_line.trim().starts_with("sha256 \""),
            "missing checksum for {target}"
        );
        replacements.push((index, archive));
    }
    if next < current.unwrap() {
        return Ok(formula.to_owned());
    }
    for (index, archive) in replacements {
        let path = dist.join(&archive);
        let bytes = std::fs::read(&path).with_context(|| format!("reading {}", path.display()))?;
        let hash = format!("{:x}", Sha256::digest(&bytes));
        let sidecar = dist.join(format!("{archive}.sha256"));
        let checksum = std::fs::read_to_string(&sidecar)
            .with_context(|| format!("reading {}", sidecar.display()))?;
        let fields: Vec<_> = checksum.split_whitespace().collect();
        ensure!(
            fields == [hash.as_str(), archive.as_str()],
            "checksum mismatch for {archive}"
        );
        let indent = &lines[index][..lines[index].len() - lines[index].trim_start().len()];
        let url = format!("{indent}url \"{BASE}{tag}/{archive}\"");
        let sha = format!("{indent}sha256 \"{hash}\"");
        lines[index] = url;
        lines[index + 1] = sha;
    }
    Ok(format!("{}\n", lines.join("\n")))
}

#[allow(dead_code)] // Also included by the CLI integration tests.
fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().collect();
    if args.len() != 3 {
        bail!("usage: update_homebrew <tag> <artifact-directory>");
    }
    let path = Path::new("Formula/skl.rb");
    let original = std::fs::read_to_string(path).context("reading Formula/skl.rb")?;
    let updated = update(&original, &args[1], Path::new(&args[2]))?;
    if updated != original {
        // A disposable workflow checkout, never user configuration.
        std::fs::write(path, updated).context("writing Formula/skl.rb")?;
    }
    Ok(())
}
