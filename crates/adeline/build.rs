//! Builds the app icon from Adeline's portrait: a Windows icon resource linked
//! into the executable, and the macOS `.icns` that `scripts/macos-app` bundles.
#![expect(
    clippy::unnecessary_debug_formatting,
    reason = "`{:?}` writes the icon path as a quoted, escaped string for the resource script"
)]
use std::{env, fs, path::PathBuf};

const PORTRAIT: &str = "../../assets/adeline.close.up.svg";

fn main() {
    println!("cargo:rerun-if-changed={PORTRAIT}");
    build_app_icon(&PathBuf::from(env::var("OUT_DIR").unwrap()));
}

// Render every icon size directly from the portrait, preserving its colors and alpha.
fn build_app_icon(out_dir: &std::path::Path) {
    let svg = fs::read(PORTRAIT).expect("Adeline close-up portrait");
    let tree = resvg::usvg::Tree::from_data(&svg, &resvg::usvg::Options::default())
        .expect("valid Adeline SVG");
    let sizes = [16_u16, 20, 24, 32, 40, 48, 64, 128, 256];
    let mut images = Vec::new();
    for size in sizes {
        images.push(render_icon(&tree, size));
    }
    if env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        // `scripts/macos-app` puts this in the app bundle. Each entry is a type code, its
        // length with the 8-byte header, and one PNG.
        let mut entries = Vec::new();
        for (kind, size) in [
            (b"icp4", 16),
            (b"icp5", 32),
            (b"ic07", 128),
            (b"ic08", 256),
            (b"ic09", 512),
            (b"ic10", 1024),
        ] {
            let png = render_icon(&tree, size);
            entries.extend_from_slice(kind);
            entries.extend_from_slice(&(u32::try_from(png.len()).unwrap() + 8).to_be_bytes());
            entries.extend_from_slice(&png);
        }
        let mut icns = b"icns".to_vec();
        icns.extend_from_slice(&(u32::try_from(entries.len()).unwrap() + 8).to_be_bytes());
        icns.extend_from_slice(&entries);
        fs::write(out_dir.join("adeline.icns"), icns).unwrap();
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
