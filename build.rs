#![expect(
    clippy::unnecessary_debug_formatting,
    reason = "`{:?}` writes each path as a quoted, escaped Rust string literal for `include_*!`"
)]
use std::{env, fmt::Write as _, fs, path::PathBuf};

fn main() {
    println!("cargo:rerun-if-changed=assets");
    let mut source =
        String::from("pub fn embedded(path: &str) -> Option<&'static [u8]> { match path {\n");
    for entry in fs::read_dir("assets").unwrap() {
        let path = entry.unwrap().path();
        if path.is_file() {
            let name = path.file_name().unwrap().to_str().unwrap();
            writeln!(
                source,
                "{name:?} => Some(include_bytes!({:?})),",
                fs::canonicalize(&path).unwrap()
            )
            .unwrap();
        }
    }
    source.push_str("_ => None } }");
    fs::write(
        PathBuf::from(env::var("OUT_DIR").unwrap()).join("assets.rs"),
        source,
    )
    .unwrap();

    println!("cargo:rerun-if-changed=bundled-themes");
    let mut paths: Vec<_> = fs::read_dir("bundled-themes")
        .expect("bundled themes directory")
        .map(|entry| entry.unwrap().path())
        .filter(|path| {
            path.is_file()
                && path.extension().is_some_and(|ext| {
                    ext.eq_ignore_ascii_case("yml") || ext.eq_ignore_ascii_case("yaml")
                })
        })
        .collect();
    paths.sort();
    let mut themes = String::from("const BUNDLED_THEMES: &[(&str, &str)] = &[\n");
    for path in paths {
        writeln!(
            themes,
            "({:?}, include_str!({:?})),",
            path.file_name().unwrap().to_str().unwrap(),
            fs::canonicalize(&path).unwrap()
        )
        .unwrap();
    }
    themes.push_str("];\n");
    fs::write(
        PathBuf::from(env::var("OUT_DIR").unwrap()).join("themes.rs"),
        themes,
    )
    .unwrap();
}
