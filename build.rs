use std::{env, fs, path::PathBuf};

fn main() {
    println!("cargo:rerun-if-changed=assets");
    let mut source =
        String::from("pub fn embedded(path: &str) -> Option<&'static [u8]> { match path {\n");
    for entry in fs::read_dir("assets").unwrap() {
        let path = entry.unwrap().path();
        if path.is_file() {
            let name = path.file_name().unwrap().to_str().unwrap();
            source.push_str(&format!(
                "{name:?} => Some(include_bytes!({:?})),\n",
                fs::canonicalize(&path).unwrap()
            ));
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
        themes.push_str(&format!(
            "({:?}, include_str!({:?})),\n",
            path.file_name().unwrap().to_str().unwrap(),
            fs::canonicalize(&path).unwrap()
        ));
    }
    themes.push_str("];\n");
    fs::write(
        PathBuf::from(env::var("OUT_DIR").unwrap()).join("themes.rs"),
        themes,
    )
    .unwrap();
}
