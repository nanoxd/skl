//! Read-only skills.sh catalog search, independent of local configuration.
use std::time::Duration;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::json;

const ENDPOINT: &str = "https://skills.sh/api/search";
const MAX_BODY: u64 = 1024 * 1024;

#[derive(Deserialize)]
struct Response {
    skills: Vec<RemoteSkill>,
}

#[derive(Deserialize)]
struct RemoteSkill {
    name: String,
    source: String,
    installs: u64,
}

#[derive(Serialize)]
struct Skill {
    name: String,
    source: String,
    installs: u64,
    install_command: String,
}

fn valid_owner(owner: &str) -> bool {
    !owner.is_empty()
        && owner.len() <= 39
        && owner.as_bytes()[0].is_ascii_alphanumeric()
        && owner
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-')
}

fn valid_source(source: &str) -> bool {
    let Some((owner, repo)) = source.split_once('/') else {
        return false;
    };
    valid_owner(owner)
        && !repo.is_empty()
        && repo.len() <= 100
        && repo != "."
        && repo != ".."
        && repo
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
}

pub fn run(query: &[String], owner: Option<&str>, limit: u16, as_json: bool) -> Result<bool> {
    let query = query.join(" ").trim().to_owned();
    if query.is_empty() || query.len() > 1024 {
        bail!("search query must contain 1–1024 bytes");
    }
    if !(1..=100).contains(&limit) {
        bail!("search --limit must be between 1 and 100");
    }
    let owner = owner.map(|owner| owner.trim().to_ascii_lowercase());
    if owner.as_deref().is_some_and(|owner| !valid_owner(owner)) {
        bail!("search --owner must be a valid GitHub owner");
    }
    let endpoint = match std::env::var("SKL_SEARCH_URL") {
        Ok(endpoint) => endpoint,
        Err(std::env::VarError::NotPresent) => ENDPOINT.into(),
        Err(_) => bail!("SKL_SEARCH_URL must be valid UTF-8"),
    };
    if !endpoint.starts_with("https://") && !endpoint.starts_with("http://") {
        bail!("SKL_SEARCH_URL must be an HTTP or HTTPS search endpoint");
    }
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(15)))
        .max_redirects(3)
        .build()
        .into();
    let mut request = agent
        .get(&endpoint)
        .header("Accept", "application/json")
        .query("q", &query)
        .query("limit", limit.to_string());
    if let Some(owner) = &owner {
        request = request.query("owner", owner);
    }
    let mut response = request.call().context("search request failed")?;
    if !response.status().is_success() {
        bail!(
            "search endpoint returned HTTP {}",
            response.status().as_u16()
        );
    }
    let body = response
        .body_mut()
        .with_config()
        .limit(MAX_BODY)
        .read_to_vec()
        .context("could not read search response (maximum 1 MiB)")?;
    let response: Response = serde_json::from_slice(&body)
        .map_err(|error| anyhow::anyhow!("invalid search response at line {}, column {}: expected skills with name, source and nonnegative integer installs", error.line(), error.column()))?;
    let mut skills = Vec::new();
    for skill in response.skills {
        // Never echo arbitrary remote terminal sequences or turn them into commands.
        // Validate the original identity instead of sanitizing it into a different skill.
        if !valid_source(&skill.source)
            || skill.name.len() > 200
            || crate::manifest::validate_name(&skill.name).is_err()
        {
            bail!(
                "invalid search result: expected a GitHub owner/repository and a valid skill name"
            );
        }
        if owner.as_ref().is_some_and(|owner| {
            !skill
                .source
                .split('/')
                .next()
                .unwrap()
                .eq_ignore_ascii_case(owner)
        }) {
            continue;
        }
        skills.push(Skill {
            install_command: format!("skl add {} --skill={}", skill.source, skill.name),
            name: skill.name,
            source: skill.source,
            installs: skill.installs,
        });
    }
    skills.sort_by(|a, b| {
        b.installs
            .cmp(&a.installs)
            .then_with(|| a.source.cmp(&b.source))
            .then_with(|| a.name.cmp(&b.name))
    });
    skills.truncate(usize::from(limit));
    if as_json {
        crate::json::emit(
            "search",
            "ok",
            json!({
                "query": query, "owner": owner, "limit": limit, "skills": skills,
            }),
            json!([]),
        )?;
    } else if skills.is_empty() {
        println!("No skills found.");
    } else {
        for skill in skills {
            println!(
                "{}/{} ({} installs)\n  {}",
                skill.source, skill.name, skill.installs, skill.install_command
            );
        }
    }
    Ok(true)
}
