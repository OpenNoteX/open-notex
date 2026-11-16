//! The palette.
//!
//! Three seeds — background, ink, accent — and everything else is derived by
//! `Kit::from_palette`, so light and dark cannot drift apart and every derived
//! colour keeps its contrast. The focus ring is the accent, not the default
//! blue, so the app has one colour rather than two.

use rune::oklch;
use rune_kit::{Kit, PaletteFocus, PaletteSeed};

/// Paper, warm off-white: a writing surface rather than a sheet of glass.
const LIGHT_BG: (f32, f32, f32) = (0.985, 0.003, 90.0);
const LIGHT_FG: (f32, f32, f32) = (0.24, 0.008, 80.0);
/// Teal ink. Reads as "notes" without the urgency of a red or the neutrality of
/// a blue, and holds 4.5:1 against both seeds.
const LIGHT_ACCENT: (f32, f32, f32) = (0.54, 0.105, 178.0);

const DARK_BG: (f32, f32, f32) = (0.185, 0.004, 80.0);
const DARK_FG: (f32, f32, f32) = (0.945, 0.004, 90.0);
const DARK_ACCENT: (f32, f32, f32) = (0.755, 0.105, 178.0);

pub fn kit(dark: bool) -> Kit {
    let (bg, fg, accent) = if dark {
        (DARK_BG, DARK_FG, DARK_ACCENT)
    } else {
        (LIGHT_BG, LIGHT_FG, LIGHT_ACCENT)
    };

    Kit::from_palette_focus(
        PaletteSeed {
            bg: oklch(bg.0, bg.1, bg.2),
            fg: oklch(fg.0, fg.1, fg.2),
            accent: oklch(accent.0, accent.1, accent.2),
        },
        PaletteFocus::Accent,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn light_and_dark_are_the_right_way_round() {
        let light = kit(false);
        let dark = kit(true);
        assert!(!light.dark);
        assert!(dark.dark);
        assert!(light.c.page.luminance() > dark.c.page.luminance());
        assert!(light.c.text_base.luminance() < dark.c.text_base.luminance());
    }

    #[test]
    fn the_accent_is_readable_on_both_backgrounds() {
        for dark in [false, true] {
            let k = kit(dark);
            let accent = k.c.bg_accent.luminance();
            let page = k.c.page.luminance();
            let ratio = (accent.max(page) + 0.05) / (accent.min(page) + 0.05);
            assert!(ratio >= 3.0, "accent on page is {ratio:.2}:1");
        }
    }

    #[test]
    fn secondary_text_stays_above_three_to_one() {
        for dark in [false, true] {
            let k = kit(dark);
            for (name, ink) in [("muted", k.c.text_muted), ("faint", k.c.text_faint)] {
                let ratio = (ink.luminance().max(k.c.page.luminance()) + 0.05)
                    / (ink.luminance().min(k.c.page.luminance()) + 0.05);
                assert!(ratio >= 3.0, "{name} on page is {ratio:.2}:1");
            }
        }
    }
}
