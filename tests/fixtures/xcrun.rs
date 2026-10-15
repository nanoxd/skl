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
    let root = env::var_os("FAKE_XCODE").unwrap();
    let root = Path::new(&root);
    fs::write(root.join("called"), "yes").unwrap();
    if root.join("fail").exists() { eprintln!("Xcode setup incomplete"); std::process::exit(1); }
    let args: Vec<_> = env::args().skip(1).collect();
    if args == ["xcodebuild", "-version"] {
        println!("{}", fs::read_to_string(root.join("version")).unwrap());
    } else {
        assert_eq!(&args[..3], &["agent", "skills", "export"]);
        assert_eq!(args.len(), 4);
        copy(&root.join("export"), Path::new(&args[3]));
    }
}
