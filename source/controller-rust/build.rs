use std::{env, fs, path::PathBuf};
fn main() {
    let root = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap()).join("../..");
    let public = root.join("public");
    println!("cargo:rerun-if-changed={}", public.display());
    println!(
        "cargo:rerun-if-changed={}",
        root.join("app/view.html").display()
    );
    let mut entries = Vec::new();
    for file in fs::read_dir(public.join("assets")).unwrap() {
        let path = file.unwrap().path();
        if !path.is_file() {
            continue;
        }
        let name = path.file_name().unwrap().to_str().unwrap();
        let mime = match path.extension().and_then(|v| v.to_str()) {
            Some("js") => "text/javascript; charset=utf-8",
            Some("css") => "text/css; charset=utf-8",
            Some("svg") => "image/svg+xml",
            Some("json") => "application/json",
            Some("md") => "text/plain; charset=utf-8",
            _ => continue,
        };
        assert!(
            name.bytes()
                .all(|v| v.is_ascii_alphanumeric() || b".-_".contains(&v))
        );
        entries.push((format!("/assets/{name}"), mime, path));
    }
    entries.push((
        "/favicon.svg".into(),
        "image/svg+xml",
        public.join("favicon.svg"),
    ));
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    let mut out = format!(
        "pub const VIEW:&str=include_str!({:?});\npub static ASSETS:&[(&str,&str,&[u8])]=&[\n",
        root.join("app/view.html").canonicalize().unwrap()
    );
    for (name, mime, path) in entries {
        out.push_str(&format!(
            "({name:?},{mime:?},include_bytes!({:?})),\n",
            path.canonicalize().unwrap()
        ));
    }
    out.push_str("];\n");
    fs::write(
        PathBuf::from(env::var("OUT_DIR").unwrap()).join("web_assets.rs"),
        out,
    )
    .unwrap();
}
