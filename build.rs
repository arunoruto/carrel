//! Embeds the web UI's built files (`web/dist`, or `$CARREL_WEB_DIST`) into the binary as `FILES`. Without
//! them carrel still builds, and `carrel web` serves only the API.

use std::path::{Path, PathBuf};
use std::{env, fs};

fn main() {
    println!("cargo:rerun-if-env-changed=CARREL_WEB_DIST");
    let dist = env::var_os("CARREL_WEB_DIST").map_or_else(|| PathBuf::from("web/dist"), PathBuf::from);
    // A missing folder would rebuild every time; watching web/ notices when dist appears.
    let watch = if dist.exists() || !Path::new("web").exists() { dist.clone() } else { "web".into() };
    println!("cargo:rerun-if-changed={}", watch.display());
    let mut files = vec![];
    walk(&dist, &dist, &mut files);
    files.sort();
    let entries: String =
        files.iter().map(|(name, path)| format!("    ({name:?}, include_bytes!({path:?})),\n")).collect();
    let out = PathBuf::from(env::var_os("OUT_DIR").expect("cargo sets OUT_DIR")).join("web.rs");
    fs::write(out, format!("pub static FILES: &[(&str, &[u8])] = &[\n{entries}];\n")).expect("writing web.rs");
}

fn walk(root: &Path, dir: &Path, files: &mut Vec<(String, String)>) {
    for entry in fs::read_dir(dir).into_iter().flatten().flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk(root, &path, files);
        } else if let (Ok(name), Ok(full)) = (path.strip_prefix(root), path.canonicalize()) {
            let name = name.components().map(|c| c.as_os_str().to_string_lossy()).collect::<Vec<_>>().join("/");
            files.push((name, full.to_string_lossy().into_owned()));
        }
    }
}
