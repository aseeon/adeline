//! Pixel facts measured inside an element's bounds, shared by the GPU tests in
//! `pixels.rs` and the Windows smoke test in `pixels_windows.rs`. No golden images:
//! fonts and antialiasing differ between OSes and drivers.
use adeline_ui::control;
use gpui_kit::base::test_support::ElementSnapshot;
use gpui_kit::{Hsla, Rgba as Color};
use image::{Rgba, RgbaImage};
use std::collections::HashMap;
use std::path::PathBuf;

// How far apart two channels may be and still count as one color.
const SAME: u8 = 6;

// The open demo chat's TODO button, which shows its current step as text.
pub const DEMO_TODO: &str = "TODO: Check matches across two messages, 3 of 4";

pub struct Frame {
    pub image: RgbaImage,
    pub scale: f32,
    pub test: &'static str,
}

impl Frame {
    pub fn region(&self, element: &ElementSnapshot) -> RgbaImage {
        control::crop(&self.image, element.bounds(), self.scale).unwrap_or_else(|| {
            self.fail(&format!(
                "{:?} lies outside the frame",
                element.label().unwrap_or_default()
            ))
        })
    }

    /// Saves the frame under `target/pixel-failures` and fails with its path.
    pub fn fail(&self, message: &str) -> ! {
        let folder = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
            .parent()
            .expect("target folder")
            .join("pixel-failures");
        std::fs::create_dir_all(&folder).expect("create pixel-failures");
        let path = folder.join(format!("{}.png", self.test));
        self.image.save(&path).expect("save the failed frame");
        panic!("{message}. Frame: {}", path.display());
    }

    pub fn check(&self, ok: bool, message: impl FnOnce() -> String) {
        if !ok {
            self.fail(&message());
        }
    }
}

#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "a channel clamped to 0..=255"
)]
pub fn rgb(color: Hsla) -> Rgba<u8> {
    let color = Color::from(color);
    let channel = |value: f32| (value.clamp(0., 1.) * 255.).round() as u8;
    Rgba([channel(color.r), channel(color.g), channel(color.b), 255])
}

pub fn same(a: Rgba<u8>, b: Rgba<u8>) -> bool {
    a.0[..3]
        .iter()
        .zip(&b.0[..3])
        .all(|(a, b)| a.abs_diff(*b) <= SAME)
}

/// The color most pixels share.
pub fn dominant(image: &RgbaImage) -> Rgba<u8> {
    let mut counts: HashMap<[u8; 4], usize> = HashMap::new();
    for pixel in image.pixels() {
        *counts.entry(pixel.0).or_default() += 1;
    }
    let (color, _) = counts
        .into_iter()
        .max_by_key(|(_, count)| *count)
        .expect("a region has pixels");
    Rgba(color)
}

/// How many pixels differ from `background`, such as text or an icon on it.
pub fn drawn_on(image: &RgbaImage, background: Rgba<u8>) -> usize {
    image
        .pixels()
        .filter(|pixel| !same(**pixel, background))
        .count()
}

/// Facts every renderer must show in the demo's chat view: the theme background,
/// text on the TODO button and the icon of the main menu.
pub fn check_demo(frame: &Frame, find: &mut dyn FnMut(&str) -> ElementSnapshot, background: Hsla) {
    let background = rgb(background);
    let todo = frame.region(&find(DEMO_TODO));
    let around = dominant(&todo);
    frame.check(drawn_on(&todo, around) > 200, || {
        "The TODO button shows no text".to_owned()
    });

    let menu = frame.region(&find("Main menu"));
    let behind = dominant(&menu);
    frame.check(drawn_on(&menu, behind) > 20, || {
        "The main menu button shows no icon".to_owned()
    });

    let whole = dominant(&frame.image);
    frame.check(same(whole, background), || {
        format!("Most of the window is {whole:?}, the theme background is {background:?}")
    });
}
