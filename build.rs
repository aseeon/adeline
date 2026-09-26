#![expect(
    clippy::unnecessary_debug_formatting,
    reason = "`{:?}` writes each path as a quoted, escaped Rust string literal for `include_*!`"
)]
use std::{env, fmt::Write as _, fs, path::PathBuf};

fn main() {
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    build_app_icon(&out_dir);
    println!("cargo:rerun-if-changed=assets");
    let mut source =
        String::from("pub fn embedded(path: &str) -> Option<&'static [u8]> { match path {\n");
    // One raster per physical pixel size covers Windows scaling from 100% to 500%.
    for size in 24..=120 {
        let name = format!("adeline-titlebar-{size}.png");
        writeln!(
            source,
            "{name:?} => Some(include_bytes!({:?})),",
            out_dir.join(&name)
        )
        .unwrap();
    }
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

// Render every icon size directly from the portrait, preserving its colors and alpha.
fn build_app_icon(out_dir: &std::path::Path) {
    let svg = fs::read("assets/adeline.close.up.svg").expect("Adeline close-up portrait");
    let tree = resvg::usvg::Tree::from_data(&svg, &resvg::usvg::Options::default())
        .expect("valid Adeline SVG");
    let sizes = [16_u16, 20, 24, 32, 40, 48, 64, 128, 256];
    let mut images = Vec::new();
    for size in 24..=120 {
        fs::write(
            out_dir.join(format!("adeline-titlebar-{size}.png")),
            render_icon(&tree, size),
        )
        .unwrap();
    }
    for size in sizes {
        images.push(render_icon(&tree, size));
    }
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }

    // Windows ICO directory followed by one PNG for each supported size.
    let count = u16::try_from(sizes.len()).unwrap();
    let mut ico = vec![0, 0, 1, 0];
    ico.extend_from_slice(&count.to_le_bytes());
    let mut offset = 6 + 16 * u32::from(count);
    for (size, png) in sizes.iter().zip(&images) {
        let dimension = u8::try_from(*size).unwrap_or(0); // 0 denotes 256 pixels.
        ico.extend_from_slice(&[dimension, dimension, 0, 0, 1, 0, 32, 0]);
        let length = u32::try_from(png.len()).unwrap();
        ico.extend_from_slice(&length.to_le_bytes());
        ico.extend_from_slice(&offset.to_le_bytes());
        offset += length;
    }
    for png in images {
        ico.extend_from_slice(&png);
    }
    let icon_path = out_dir.join("adeline.ico");
    fs::write(&icon_path, ico).unwrap();
    let resource = out_dir.join("adeline.rc");
    // GPUI loads icon resource 1 for its Windows window class and taskbar icon.
    fs::write(&resource, format!("1 ICON {icon_path:?}\n")).unwrap();
    embed_resource::compile(&resource, embed_resource::NONE)
        .manifest_required()
        .expect("embed Adeline Windows icon");
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
