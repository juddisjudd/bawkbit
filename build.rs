use std::fmt::Write as _;
use std::path::PathBuf;

fn main() {
    const ICON: &str = "assets/brand/bawkbit.ico";
    println!("cargo:rerun-if-changed={ICON}");
    let windows = std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows");
    if windows && std::path::Path::new(ICON).exists() {
        let mut resource = winresource::WindowsResource::new();
        resource
            .set_icon(ICON)
            .set("FileDescription", "bawkbit")
            .set("ProductName", "bawkbit");
        resource.compile().expect("compile Windows resources");
    }
    embed_flags();
}

/// The web UI's country flags, embedded for the peers list.
fn embed_flags() {
    const FLAGS: &str = "web/public/flags";
    println!("cargo:rerun-if-changed={FLAGS}");
    let mut flags: Vec<(String, PathBuf)> = std::fs::read_dir(FLAGS)
        .map(|entries| {
            entries
                .flatten()
                .map(|entry| entry.path())
                .filter_map(|path| {
                    let code = path.file_stem()?.to_str()?.to_string();
                    let svg = path.extension().is_some_and(|ext| ext == "svg");
                    let path = path.canonicalize().ok()?;
                    (svg && code.len() == 2).then_some((code, path))
                })
                .collect()
        })
        .unwrap_or_default();
    flags.sort();
    let mut out = String::from("pub static FLAGS: &[(&str, &[u8])] = &[\n");
    for (code, path) in &flags {
        writeln!(
            out,
            "    ({code:?}, include_bytes!({:?})),",
            path.display().to_string()
        )
        .unwrap();
    }
    out.push_str("];\n");
    let target = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("flags.rs");
    std::fs::write(target, out).unwrap();
}
