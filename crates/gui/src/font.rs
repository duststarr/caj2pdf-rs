//! Locate and load a CJK-capable system font so egui can render
//! Chinese (and Japanese / Korean) text.
//!
//! egui's default font (Inter-style ASCII) does not include CJK
//! glyphs; without this loader, every Chinese character renders as a
//! missing-glyph box. We don't bundle a CJK font because the
//! canonical ones (Noto Sans CJK, Source Han Sans, PingFang) are
//! 10–20 MB and would dwarf the actual application code.
//!
//! The loader tries, in order:
//!
//! 1. The `CAJ2PDF_FONT` environment variable (a path to any .ttf/.ttc
//!    / .otf). Useful for users with fonts in non-standard locations.
//! 2. Linux: `/usr/share/fonts/opentype/noto/NotoSansCJK-*.ttc`,
//!    `/usr/share/fonts/truetype/wqy/wqy-zenhei.ttc`.
//! 3. macOS: `/System/Library/Fonts/PingFang.ttc`,
//!    `/System/Library/Fonts/STHeiti*.ttc`,
//!    `/Library/Fonts/PingFang.ttc`.
//! 4. Windows: `C:\Windows\Fonts\msyh.ttc`, `msyh.ttf`, `simsun.ttc`.
//!
//! If nothing is found the GUI still works — text just shows missing
//! glyphs.

use std::fs;
use std::path::PathBuf;

use eframe::egui::{self, FontData, FontDefinitions, FontFamily};

/// Return a `FontDefinitions` that includes a CJK font if one is
/// available on the system.
///
/// The returned value can be passed to `egui::Context::set_fonts(...)`
/// from inside `eframe::CreationContext`.
pub fn cjk_font_definitions() -> FontDefinitions {
    let mut fonts = FontDefinitions::default();

    if let Some(path) = find_cjk_font() {
        match fs::read(&path) {
            Ok(bytes) => {
                tracing::info!(font = %path.display(), "loaded CJK font for GUI");
                let font_name = "cjk".to_owned();
                fonts.font_data.insert(
                    font_name.clone(),
                    FontData::from_owned(bytes).into(),
                );
                // Make the CJK font the *primary* proportional font so
                // it covers the Latin glyphs too — that way we don't
                // have to merge into the existing family.
                if let Some(family) = fonts.families.get_mut(&FontFamily::Proportional) {
                    family.insert(0, font_name.clone());
                }
                if let Some(family) = fonts.families.get_mut(&FontFamily::Monospace) {
                    family.insert(0, font_name);
                }
            }
            Err(e) => {
                tracing::warn!(font = %path.display(), error = %e,
                    "found CJK font but could not read it");
            }
        }
    } else {
        tracing::warn!(
            "no CJK font found on the system — Chinese text will show as missing glyphs. \
             Set CAJ2PDF_FONT to the path of a .ttf/.ttc/.otf to override the search path."
        );
    }

    fonts
}

/// True if egui is currently hovering in a drag-and-drop "files"
/// state. Used to highlight the drop zone.
pub fn is_being_dragged(ctx: &egui::Context) -> bool {
    !ctx.input(|i| i.raw.dropped_files.is_empty())
        || ctx.input(|i| i.raw.hovered_files.iter().any(|f| f.path.is_some()))
}

fn find_cjk_font() -> Option<PathBuf> {
    if let Ok(path) = std::env::var("CAJ2PDF_FONT") {
        let p = PathBuf::from(path);
        if p.is_file() {
            return Some(p);
        }
    }
    for candidate in candidate_paths() {
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

fn candidate_paths() -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = Vec::new();

    if cfg!(target_os = "linux") {
        v.extend(LINUX_CANDIDATES.iter().map(PathBuf::from));
    } else if cfg!(target_os = "macos") {
        v.extend(MAC_CANDIDATES.iter().map(PathBuf::from));
    } else if cfg!(target_os = "windows") {
        v.extend(WIN_CANDIDATES.iter().map(PathBuf::from));
    }
    // Fallback paths that are common across distros / desktop envs.
    v.extend(FALLBACK_CANDIDATES.iter().map(PathBuf::from));

    v
}

const LINUX_CANDIDATES: &[&str] = &[
    "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
    "/usr/share/fonts/opentype/noto/NotoSansCJK-Medium.ttc",
    "/usr/share/fonts/opentype/noto/NotoSansCJK-Bold.ttc",
    "/usr/share/fonts/truetype/wqy/wqy-zenhei.ttc",
    "/usr/share/fonts/truetype/wqy/wqy-microhei.ttc",
    "/usr/share/fonts/truetype/arphic/uming.ttc",
    "/usr/share/fonts/truetype/arphic/ukai.ttc",
];

const MAC_CANDIDATES: &[&str] = &[
    "/System/Library/Fonts/PingFang.ttc",
    "/System/Library/Fonts/STHeiti Light.ttc",
    "/System/Library/Fonts/STHeiti Medium.ttc",
    "/System/Library/Fonts/STHeiti Medium.ttc",
    "/Library/Fonts/PingFang.ttc",
    "/System/Library/Fonts/Hiragino Sans GB.ttc",
];

const WIN_CANDIDATES: &[&str] = &[
    "C:\\Windows\\Fonts\\msyh.ttc",
    "C:\\Windows\\Fonts\\msyh.ttf",
    "C:\\Windows\\Fonts\\msyhbd.ttc",
    "C:\\Windows\\Fonts\\simsun.ttc",
    "C:\\Windows\\Fonts\\simsunb.ttf",
    "C:\\Windows\\Fonts\\simhei.ttf",
];

const FALLBACK_CANDIDATES: &[&str] = &[
    "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
];

#[cfg(test)]
mod tests {
    use super::*;

    /// The font loader must return a `FontDefinitions` even when no
    /// CJK font is installed — it should fall back to the egui
    /// default font and emit a warning. The result must always be
    /// serializable enough for `Context::set_fonts`.
    #[test]
    fn font_definitions_returns_something() {
        let fds = cjk_font_definitions();
        // Whether or not a CJK font was found, the default
        // Proportional family must still be present.
        assert!(fds.families.contains_key(&FontFamily::Proportional));
        assert!(fds.families.contains_key(&FontFamily::Monospace));
    }

    /// `is_being_dragged` must be callable without panicking even
    /// with an empty Context (no inputs, no dropped files).
    #[test]
    fn is_being_dragged_on_empty_context() {
        let ctx = egui::Context::default();
        assert!(!is_being_dragged(&ctx));
    }
}
