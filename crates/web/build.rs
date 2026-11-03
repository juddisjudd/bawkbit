use std::fmt::Write as _;
use std::path::{Path, PathBuf};

/// Embeds the built web UI from `web/dist`, or nothing when it is not built yet.
fn main() {
    let root = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let dist = root.join("../../web/dist");
    println!("cargo:rerun-if-changed={}", dist.display());

    let mut files = Vec::new();
    if dist.is_dir() {
        collect(&dist, &dist, &mut files);
    }
    files.sort();

    let mut out = String::from("pub static FILES: &[(&str, &[u8])] = &[\n");
    for (name, path) in &files {
        println!("cargo:rerun-if-changed={}", path.display());
        writeln!(
            out,
            "    ({name:?}, include_bytes!({:?})),",
            path.display().to_string()
        )
        .unwrap();
    }
    out.push_str("];\n");
    let target = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("webui.rs");
    std::fs::write(target, out).unwrap();
}

fn collect(base: &Path, dir: &Path, files: &mut Vec<(String, PathBuf)>) {
    println!("cargo:rerun-if-changed={}", dir.display());
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect(base, &path, files);
        } else if let Ok(relative) = path.strip_prefix(base) {
            let name = relative.to_string_lossy().replace('\\', "/");
            let path = path.canonicalize().unwrap_or(path);
            files.push((name, path));
        }
    }
}
