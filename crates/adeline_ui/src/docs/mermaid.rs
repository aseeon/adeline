//! Offline Mermaid rendering: `mermaid-rs-renderer` lays the diagram out as
//! SVG, and resvg rasterizes it to a PNG at 2x for high-DPI screens.
use std::sync::{Arc, LazyLock};

use gpui_kit::{Image, ImageFormat};
use mermaid_rs_renderer::{RenderOptions, Theme, render_strict};
use resvg::{tiny_skia, usvg};

const MAX_SOURCE_BYTES: usize = 100 * 1024;
const MAX_PIXELS: u64 = 4096 * 4096;
const SCALE: f32 = 2.0;

static FONTS: LazyLock<Arc<usvg::fontdb::Database>> = LazyLock::new(|| {
    let mut db = usvg::fontdb::Database::new();
    db.load_system_fonts();
    Arc::new(db)
});

pub(crate) fn render(source: &str, dark: bool) -> Result<Arc<Image>, String> {
    if source.len() > MAX_SOURCE_BYTES {
        return Err("diagram source is over 100 KB".into());
    }
    let theme = if dark { Theme::dark() } else { Theme::modern() };
    let options = RenderOptions {
        theme,
        ..RenderOptions::default()
    };
    let svg = render_strict(source, options).map_err(|e| e.to_string())?;

    let tree = usvg::Tree::from_str(
        &svg,
        &usvg::Options {
            fontdb: Arc::clone(&FONTS),
            ..usvg::Options::default()
        },
    )
    .map_err(|e| format!("invalid SVG: {e}"))?;
    let size = tree
        .size()
        .to_int_size()
        .scale_by(SCALE)
        .ok_or("diagram is empty")?;
    if u64::from(size.width()) * u64::from(size.height()) > MAX_PIXELS {
        return Err("diagram is too large".into());
    }
    let mut pixmap =
        tiny_skia::Pixmap::new(size.width(), size.height()).ok_or("diagram is empty")?;
    resvg::render(
        &tree,
        tiny_skia::Transform::from_scale(SCALE, SCALE),
        &mut pixmap.as_mut(),
    );
    let png = pixmap.encode_png().map_err(|e| e.to_string())?;
    Ok(Arc::new(Image::from_bytes(ImageFormat::Png, png)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_sources_give_a_reason() {
        for source in [
            "flowchart LR\n  subgraph one\n  A --> B",
            "notADiagram\n  A --> B",
        ] {
            let reason = render(source, false).expect_err(source);
            assert!(!reason.trim().is_empty(), "{source}");
        }
    }

    #[test]
    fn oversized_diagrams_are_refused() {
        let source = format!("flowchart LR\n  A[{}]", "x".repeat(MAX_SOURCE_BYTES));
        assert_eq!(
            render(&source, false).unwrap_err(),
            "diagram source is over 100 KB"
        );
        let chain: Vec<_> = (0..40).map(|i| format!("P{i}->>P{}: m", i + 1)).collect();
        let source = format!("sequenceDiagram\n{}", chain.join("\n"));
        assert_eq!(render(&source, false).unwrap_err(), "diagram is too large");
    }
}
