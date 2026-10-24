// Compiled by the CLI tests; no Xcode or shell is needed.
use std::{env, fs, path::Path};
fn copy(source: &Path, dest: &Path) {
    fs::create_dir_all(dest).unwrap();
    for entry in fs::read_dir(source).unwrap() {
        let path = entry.unwrap().path();
        let target = dest.join(path.file_name().unwrap());
        if path.is_symlink() {
            std::os::unix::fs::symlink(fs::read_link(path).unwrap(), target).unwrap();
        } else if path.is_dir() { copy(&path, &target); }
        else { fs::copy(path, target).unwrap(); }
    }
}
fn main() {
    // skl passes only DEVELOPER_DIR through, so it locates the fake Xcode's data.
    let root = env::var_os("DEVELOPER_DIR").unwrap();
    let root = Path::new(&root);
    let args: Vec<_> = env::args().skip(1).collect();
    if args == ["--find", "xcodebuild"] {
        // A Mac with only the Command Line Tools has no xcodebuild.
        if root.join("missing").exists() { eprintln!("xcrun: error: unable to find utility \"xcodebuild\""); std::process::exit(1); }
        println!("{}", root.join("xcodebuild").display());
        return;
    }
    fs::write(root.join("called"), "yes").unwrap();
    if root.join("fail").exists() { eprintln!("Xcode setup incomplete"); std::process::exit(1); }
    if args == ["xcodebuild", "-version"] {
        println!("{}", fs::read_to_string(root.join("version")).unwrap());
    } else {
        assert_eq!(&args[..3], &["agent", "skills", "export"]);
        assert_eq!(args.len(), 4);
        if root.join("temporary-files").exists() {
            // Exporters may create and remove scratch files beneath TMPDIR.
            let scratch = std::env::temp_dir().join("scratch");
            let start = std::time::Instant::now();
            while start.elapsed() < std::time::Duration::from_secs(1) {
                fs::create_dir(&scratch).unwrap();
                for i in 0..128 {
                    fs::write(scratch.join(i.to_string()), b"temporary").unwrap();
                }
                fs::remove_dir_all(&scratch).unwrap();
            }
        }
        copy(&root.join("export"), Path::new(&args[3]));
    }
}
