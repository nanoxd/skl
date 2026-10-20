use std::{fs, io::Write, path::Path, time::Duration};

fn main() {
    let args: Vec<_> = std::env::args().collect();
    let root = Path::new(&args[1]);
    fs::write(root.join("ran"), b"yes").unwrap();
    assert!(std::env::var_os("SKL_TEST_SECRET").is_none());
    assert_eq!(std::env::current_dir().unwrap(), std::path::PathBuf::from(std::env::var_os("HOME").unwrap()).canonicalize().unwrap());
    let mode = fs::read_to_string(root.join("mode")).unwrap_or_default();
    if args.get(2).is_some_and(|s| s == "--child") {
        std::thread::sleep(Duration::from_secs(10));
        return;
    }
    if args.get(2).is_some_and(|s| s == "--collection") {
        // Exports every skill under `collection`, like an app with several skills.
        fn copy(source: &Path, dest: &Path) {
            fs::create_dir_all(dest).unwrap();
            for entry in fs::read_dir(source).unwrap() {
                let path = entry.unwrap().path();
                let target = dest.join(path.file_name().unwrap());
                if path.is_dir() { copy(&path, &target); } else { fs::copy(&path, &target).unwrap(); }
            }
        }
        copy(&root.join("collection"), Path::new(&args[3]));
        return;
    }
    if args.get(2).is_some_and(|s| s == "--version") {
        if mode == "version-fail" { std::process::exit(3); }
        print!("app 1.2.3");
        return;
    }
    match mode.as_str() {
        "fail" => std::process::exit(2),
        "descendant" => {
            std::process::Command::new(std::env::current_exe().unwrap()).arg(root).arg("--child").spawn().unwrap();
            return;
        }
        "stderr" => { std::io::stderr().write_all(&vec![b'x'; 9 * 1024 * 1024]).unwrap(); return; }
        "timeout" => std::thread::sleep(Duration::from_secs(10)),
        "large" => { std::io::stdout().write_all(&vec![b'x'; 9 * 1024 * 1024]).unwrap(); return; }
        "empty" => return,
        "invalid" => { std::io::stdout().write_all(&[0xff]).unwrap(); return; }
        _ => {}
    }
    let text = fs::read(root.join("content")).unwrap();
    if let Some(dest) = args.get(2) {
        fs::create_dir(dest).unwrap();
        if mode == "symlink-root" {
            fs::remove_dir(dest).unwrap();
            std::os::unix::fs::symlink(root, dest).unwrap();
        } else if mode == "symlink" {
            std::os::unix::fs::symlink(root.join("content"), Path::new(dest).join("SKILL.md")).unwrap();
        } else {
            fs::write(Path::new(dest).join("SKILL.md"), text).unwrap();
            fs::create_dir(Path::new(dest).join("references")).unwrap();
            if mode == "escape" {
                std::os::unix::fs::symlink(root, Path::new(dest).join("references/escape")).unwrap();
            } else if mode == "fifo" {
                std::process::Command::new("/usr/bin/mkfifo").arg(Path::new(dest).join("pipe")).status().unwrap();
            } else if mode == "large-tree" {
                fs::write(Path::new(dest).join("large"), vec![0; 33 * 1024 * 1024]).unwrap();
            } else {
                fs::write(Path::new(dest).join("references/guide.md"), "reference").unwrap();
            }
        }
    } else {
        std::io::stdout().write_all(&text).unwrap();
    }
}
