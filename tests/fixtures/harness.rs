// Compiled by the CLI tests as both `claude` and `codex`, so plugin tests never run
// a real agent. State lives in $HOME/.fake-<program>; every call is logged there.
use std::{env, fs, path::Path, process::exit};

fn copy(source: &Path, dest: &Path) {
    fs::create_dir_all(dest).unwrap();
    for entry in fs::read_dir(source).unwrap() {
        let path = entry.unwrap().path();
        let target = dest.join(path.file_name().unwrap());
        // Follow symlinks, as the real CLIs copy a marketplace's linked folder.
        if fs::metadata(&path).unwrap().is_dir() {
            copy(&path, &target);
        } else {
            fs::copy(&path, &target).unwrap();
        }
    }
}

fn lines(path: &Path) -> Vec<String> {
    fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .map(str::to_owned)
        .collect()
}

fn write(path: &Path, lines: &[String]) {
    let mut text = lines.join("\n");
    if !text.is_empty() {
        text.push('\n');
    }
    fs::write(path, text).unwrap();
}

/// The first `"name": "..."` value, which is the marketplace's or plugin's own.
fn name_in(text: &str) -> Option<String> {
    let rest = &text[text.find("\"name\"")? + 6..];
    let rest = &rest[rest.find('"')? + 1..];
    Some(rest[..rest.find('"')?].to_owned())
}

fn main() {
    let program = env::args().next().unwrap();
    let program = Path::new(&program).file_name().unwrap().to_string_lossy().into_owned();
    let claude = program == "claude";
    let home = Path::new(&env::var_os("HOME").unwrap()).join(format!(".fake-{program}"));
    fs::create_dir_all(&home).unwrap();
    let args: Vec<String> = env::args().skip(1).collect();
    let mut log = lines(&home.join("log"));
    log.push(args.join(" "));
    write(&home.join("log"), &log);
    let marketplaces = home.join("marketplaces");
    let installed = home.join("installed");
    let words: Vec<&str> = args.iter().map(String::as_str).collect();
    match words.as_slice() {
        ["plugin", "marketplace", "list", "--json"] => {
            let entries: Vec<String> = lines(&marketplaces)
                .iter()
                .map(|line| {
                    let (name, path) = line.split_once('\t').unwrap();
                    if claude {
                        format!("{{\"name\": \"{name}\", \"source\": \"directory\", \"path\": \"{path}\"}}")
                    } else {
                        format!("{{\"name\": \"{name}\", \"root\": \"{path}\"}}")
                    }
                })
                .collect();
            if claude {
                println!("[{}]", entries.join(", "));
            } else {
                println!("{{\"marketplaces\": [{}]}}", entries.join(", "));
            }
        }
        ["plugin", "marketplace", "add", path, "--json"] => {
            let text = fs::read_to_string(Path::new(path).join(".claude-plugin/marketplace.json")).unwrap();
            let name = name_in(&text).unwrap();
            let mut known = lines(&marketplaces);
            known.retain(|line| !line.starts_with(&format!("{name}\t")));
            known.push(format!("{name}\t{path}"));
            write(&marketplaces, &known);
            println!("{{\"marketplace\": \"{name}\"}}");
        }
        ["plugin", "list", "--json"] => {
            let ids = lines(&installed);
            if claude {
                let entries: Vec<String> = ids.iter().map(|id| format!("{{\"id\": \"{id}\"}}")).collect();
                println!("[{}]", entries.join(", "));
            } else {
                let entries: Vec<String> = ids.iter().map(|id| format!("{{\"pluginId\": \"{id}\"}}")).collect();
                println!("{{\"installed\": [{}], \"available\": []}}", entries.join(", "));
            }
        }
        ["plugin", "install", id, "--scope", "user", "--json"] | ["plugin", "add", id, "--json"]
            if claude == (words[1] == "install") =>
        {
            if home.join("fail").exists() {
                eprintln!("install failed");
                exit(1);
            }
            let (name, marketplace) = id.split_once('@').unwrap();
            let root = lines(&marketplaces)
                .iter()
                .find_map(|line| line.strip_prefix(&format!("{marketplace}\t")).map(str::to_owned))
                .unwrap_or_else(|| {
                    eprintln!("unknown marketplace {marketplace}");
                    exit(1)
                });
            let folder = Path::new(&root).join(name);
            let manifest = [".codex-plugin/plugin.json", ".claude-plugin/plugin.json", ".cursor-plugin/plugin.json"]
                .iter()
                .find_map(|m| fs::read_to_string(folder.join(m)).ok());
            // Codex insists that the plugin's own name matches its marketplace entry.
            if !claude && manifest.as_deref().and_then(name_in).as_deref() != Some(name) {
                eprintln!("plugin.json name does not match marketplace plugin name `{name}`");
                exit(1);
            }
            let cache = home.join("cache").join(name);
            let _ = fs::remove_dir_all(&cache);
            copy(&folder, &cache);
            let mut ids = lines(&installed);
            if !ids.iter().any(|i| i == id) {
                ids.push(id.to_string());
            }
            write(&installed, &ids);
            println!("{{\"plugin\": \"{id}\"}}");
        }
        ["plugin", "uninstall", id, "--scope", "user", "--json"] | ["plugin", "remove", id, "--json"] => {
            let mut ids = lines(&installed);
            ids.retain(|i| i != id);
            write(&installed, &ids);
            let _ = fs::remove_dir_all(home.join("cache").join(id.split_once('@').unwrap().0));
            println!("{{\"plugin\": \"{id}\"}}");
        }
        _ => {
            eprintln!("unexpected arguments: {args:?}");
            exit(2);
        }
    }
}
