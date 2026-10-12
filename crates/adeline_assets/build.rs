#![expect(
    clippy::unnecessary_debug_formatting,
    reason = "`{:?}` writes each path as a quoted, escaped Rust string literal for `include_*!`"
)]
use std::{env, fmt::Write as _, fs, path::PathBuf};

const ASSETS: &str = "../../assets";
const THEMES: &str = "../../bundled-themes";

fn main() {
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    println!("cargo:rerun-if-changed={ASSETS}");
    let mut source =
        String::from("pub fn embedded(path: &str) -> Option<&'static [u8]> { match path {\n");
    // One raster per physical pixel size covers Windows scaling from 100% to 500%.
    let svg = fs::read(format!("{ASSETS}/adeline.close.up.svg")).expect("Adeline portrait");
    let tree = resvg::usvg::Tree::from_data(&svg, &resvg::usvg::Options::default())
        .expect("valid Adeline SVG");
    for size in 24..=120 {
        let name = format!("adeline-titlebar-{size}.png");
        let path = out_dir.join(&name);
        fs::write(&path, render_icon(&tree, size)).unwrap();
        writeln!(source, "{name:?} => Some(include_bytes!({path:?})),").unwrap();
    }
    for entry in fs::read_dir(ASSETS).unwrap() {
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
    fs::write(out_dir.join("assets.rs"), source).unwrap();

    println!("cargo:rerun-if-changed={THEMES}");
    let mut paths: Vec<_> = fs::read_dir(THEMES)
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
    let mut themes = String::from("pub const BUNDLED_THEMES: &[(&str, &str)] = &[\n");
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
    fs::write(out_dir.join("themes.rs"), themes).unwrap();
}

fn render_icon(tree: &resvg::usvg::Tree, size: u16) -> Vec<u8> {
    let mut pixmap =
        resvg::tiny_skia::Pixmap::new(u32::from(size), u32::from(size)).expect("icon canvas");
    let scale = f32::from(size) / tree.size().width();
    resvg::render(
        tree,
        resvg::tiny_skia::Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );
    pixmap.encode_png().expect("icon PNG")
}
