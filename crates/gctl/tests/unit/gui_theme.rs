#![allow(clippy::unwrap_used, clippy::expect_used)]

//! The window's two colour schemes, pinned to their hex values (each token's
//! oklch converted once, outside this repo), the desktop-mode mapping, the
//! fonts, the radius ladder, the control height and the app id.

use iced::font::Family;
use iced::theme::Mode;
use iced::{Color, Theme};

use super::*;

fn hex(color: Color) -> String {
    let [r, g, b, _] = color.into_rgba8();
    format!("#{r:02X}{g:02X}{b:02X}")
}

fn opaque(color: Color) -> bool {
    (color.a - 1.0).abs() < f32::EPSILON
}

/// Every opaque token of a scheme, by its design-token name.
fn opaque_tokens(tokens: &Tokens) -> [(&'static str, Color); 21] {
    [
        ("well", tokens.well),
        ("bg", tokens.bg),
        ("surface-1", tokens.surface_1),
        ("surface-2", tokens.surface_2),
        ("surface-3", tokens.surface_3),
        ("line", tokens.line),
        ("line-soft", tokens.line_soft),
        ("text", tokens.text),
        ("text-2", tokens.text_2),
        ("text-3", tokens.text_3),
        ("text-4", tokens.text_4),
        ("accent", tokens.accent),
        ("accent-hover", tokens.accent_hover),
        ("accent-press", tokens.accent_press),
        ("accent-text", tokens.accent_text),
        ("on-accent", tokens.on_accent),
        ("success", tokens.success),
        ("warning", tokens.warning),
        ("danger", tokens.danger),
        ("danger-text", tokens.danger_text),
        ("on-danger", tokens.on_danger),
    ]
}

fn assert_table(tokens: &Tokens, expected: [(&str, &str); 21]) {
    for ((name, color), (want_name, want_hex)) in opaque_tokens(tokens).into_iter().zip(expected) {
        assert_eq!(name, want_name);
        assert!(opaque(color), "{name} must be opaque");
        assert_eq!(hex(color), want_hex, "{name}");
    }
}

// --- the desktop's mode picks the scheme ---------------------------------------

#[test]
fn no_desktop_preference_is_dark() {
    assert_eq!(Scheme::from_mode(Mode::None), Scheme::Dark);
}

#[test]
fn a_dark_desktop_is_dark() {
    assert_eq!(Scheme::from_mode(Mode::Dark), Scheme::Dark);
}

#[test]
fn a_light_desktop_is_light() {
    assert_eq!(Scheme::from_mode(Mode::Light), Scheme::Light);
}

// --- the hex table, both schemes -----------------------------------------------

#[test]
fn dark_tokens_match_the_hex_table() {
    assert_table(
        Scheme::Dark.tokens(),
        [
            ("well", "#171923"),
            ("bg", "#20222E"),
            ("surface-1", "#2B2D3A"),
            ("surface-2", "#363847"),
            ("surface-3", "#434555"),
            ("line", "#363847"),
            ("line-soft", "#2E2F3C"),
            ("text", "#DADCF2"),
            ("text-2", "#B9BCD1"),
            ("text-3", "#A0A3B8"),
            ("text-4", "#9497AB"),
            ("accent", "#68BBEF"),
            ("accent-hover", "#77CFFF"),
            ("accent-press", "#59A8D8"),
            ("accent-text", "#77CFFF"),
            ("on-accent", "#191A24"),
            ("success", "#7CD591"),
            ("warning", "#E6C873"),
            ("danger", "#EF7D88"),
            ("danger-text", "#FF99A4"),
            ("on-danger", "#231718"),
        ],
    );
}

#[test]
fn light_tokens_match_the_hex_table() {
    assert_table(
        Scheme::Light.tokens(),
        [
            ("well", "#E4E6EC"),
            ("bg", "#EFF0F4"),
            ("surface-1", "#FDFDFF"),
            ("surface-2", "#E8E9EF"),
            ("surface-3", "#DDDFE6"),
            ("line", "#D8DAE3"),
            ("line-soft", "#E2E4EB"),
            ("text", "#2B2D38"),
            ("text-2", "#4A4C58"),
            ("text-3", "#60636E"),
            ("text-4", "#676A74"),
            ("accent", "#007AB9"),
            ("accent-hover", "#0068A3"),
            ("accent-press", "#00598E"),
            ("accent-text", "#006BA4"),
            ("on-accent", "#FCFEFF"),
            ("success", "#00792F"),
            ("warning", "#805E00"),
            ("danger", "#BD1F44"),
            ("danger-text", "#A80034"),
            ("on-danger", "#FFFDFD"),
        ],
    );
}

#[test]
fn the_accent_tints_are_the_accent_at_the_token_alpha() {
    for (scheme, quiet, line) in [(Scheme::Dark, 0.16, 0.38), (Scheme::Light, 0.12, 0.34)] {
        let tokens = scheme.tokens();
        for (name, color, alpha) in [
            ("accent-quiet", tokens.accent_quiet, quiet),
            ("accent-line", tokens.accent_line, line),
        ] {
            assert_eq!(hex(color), hex(tokens.accent), "{scheme:?} {name}");
            assert!(
                (color.a - alpha).abs() < 1e-6,
                "{scheme:?} {name} alpha {}",
                color.a
            );
        }
    }
}

#[test]
fn no_token_is_pure_white_or_pure_black() {
    for scheme in [Scheme::Dark, Scheme::Light] {
        for (name, color) in opaque_tokens(scheme.tokens()) {
            let value = hex(color);
            assert!(
                value != "#FFFFFF" && value != "#000000",
                "{scheme:?} {name} is {value}"
            );
        }
    }
}

// --- the iced theme carries the scheme -----------------------------------------

#[test]
fn each_iced_theme_carries_its_schemes_base_colours() {
    for scheme in [Scheme::Dark, Scheme::Light] {
        let tokens = scheme.tokens();
        let palette = Theme::palette(&scheme.theme());
        assert_eq!(
            hex(palette.background),
            hex(tokens.bg),
            "{scheme:?} background"
        );
        assert_eq!(hex(palette.text), hex(tokens.text), "{scheme:?} text");
        assert_eq!(
            hex(palette.primary),
            hex(tokens.accent),
            "{scheme:?} primary"
        );
        assert_eq!(
            hex(palette.success),
            hex(tokens.success),
            "{scheme:?} success"
        );
        assert_eq!(
            hex(palette.warning),
            hex(tokens.warning),
            "{scheme:?} warning"
        );
        assert_eq!(hex(palette.danger), hex(tokens.danger), "{scheme:?} danger");
    }
}

#[test]
fn the_two_iced_themes_differ() {
    assert_ne!(
        hex(Theme::palette(&Scheme::Dark.theme()).background),
        hex(Theme::palette(&Scheme::Light.theme()).background)
    );
}

// --- the modal: scrim and float shadow -----------------------------------------

#[test]
fn the_scrim_is_the_backdrop_token() {
    for (scheme, want_hex, want_alpha) in [
        (Scheme::Dark, "#0B0C16", 0.66),
        (Scheme::Light, "#525561", 0.32),
    ] {
        let tokens = scheme.tokens();
        assert_eq!(hex(tokens.backdrop), want_hex, "{scheme:?} backdrop");
        assert!(
            (tokens.backdrop.a - want_alpha).abs() < 1e-6,
            "{scheme:?} backdrop alpha {}",
            tokens.backdrop.a
        );
        let style = scrim(scheme)(&scheme.theme());
        assert_eq!(
            style.background,
            Some(iced::Background::Color(tokens.backdrop)),
            "{scheme:?} scrim"
        );
    }
}

#[test]
fn the_modal_shadow_is_the_float_token_hue_tinted() {
    for (scheme, want_hex, want_alpha) in [
        (Scheme::Dark, "#020207", 0.66),
        (Scheme::Light, "#6C7184", 0.21),
    ] {
        let tokens = scheme.tokens();
        assert_eq!(hex(tokens.shadow), want_hex, "{scheme:?} shadow");
        assert!(
            (tokens.shadow.a - want_alpha).abs() < 1e-6,
            "{scheme:?} shadow alpha {}",
            tokens.shadow.a
        );
        let shadow = modal(scheme)(&scheme.theme()).shadow;
        assert_eq!(shadow.color, tokens.shadow, "{scheme:?} modal shadow");
        assert!(
            shadow.blur_radius > 0.0 && shadow.blur_radius <= 22.0,
            "{scheme:?} blur {}",
            shadow.blur_radius
        );
    }
}

// --- disabled controls look disabled -------------------------------------------

#[test]
fn every_button_variant_has_its_own_disabled_look() {
    use iced::widget::button::Status;
    for scheme in [Scheme::Dark, Scheme::Light] {
        let theme = scheme.theme();
        for (name, style) in [
            ("filled", filled_button(scheme)),
            ("primary", primary_button(scheme)),
            ("secondary", secondary_button(scheme)),
            ("danger", danger_button(scheme)),
            ("quiet", quiet_button(scheme)),
        ] {
            assert_ne!(
                style(&theme, Status::Disabled),
                style(&theme, Status::Active),
                "{scheme:?} {name}"
            );
        }
    }
}

#[test]
fn a_disabled_text_input_has_its_own_look() {
    use iced::widget::text_input::Status;
    for scheme in [Scheme::Dark, Scheme::Light] {
        let theme = scheme.theme();
        let style = text_input(scheme);
        assert_ne!(
            style(&theme, Status::Disabled),
            style(&theme, Status::Active),
            "{scheme:?}"
        );
    }
}

#[test]
fn disabled_checkboxes_and_toggles_have_their_own_look() {
    use iced::widget::checkbox::Status as CheckStatus;
    use iced::widget::toggler::Status as ToggleStatus;
    for scheme in [Scheme::Dark, Scheme::Light] {
        let theme = scheme.theme();
        for on in [false, true] {
            let style = checkbox(scheme);
            assert_ne!(
                style(&theme, CheckStatus::Disabled { is_checked: on }),
                style(&theme, CheckStatus::Active { is_checked: on }),
                "{scheme:?} checkbox {on}"
            );
            for (name, style) in [("toggler", toggler(scheme)), ("dim", dim_toggler(scheme))] {
                assert_ne!(
                    style(&theme, ToggleStatus::Disabled { is_toggled: on }),
                    style(&theme, ToggleStatus::Active { is_toggled: on }),
                    "{scheme:?} {name} {on}"
                );
            }
        }
    }
}

#[test]
fn disabled_buttons_keep_the_designs_silhouettes() {
    use iced::Background;
    use iced::widget::button::Status;
    for scheme in [Scheme::Dark, Scheme::Light] {
        let theme = scheme.theme();
        let t = scheme.tokens();
        let filled = filled_button(scheme)(&theme, Status::Disabled);
        assert_eq!(
            filled.background,
            Some(Background::Color(t.surface_2)),
            "{scheme:?} filled"
        );
        let primary = primary_button(scheme)(&theme, Status::Disabled);
        assert_eq!(
            primary.background,
            Some(Background::Color(t.surface_2)),
            "{scheme:?} primary"
        );
        assert_eq!(primary.border.color, t.surface_3, "{scheme:?} primary edge");
        assert!(primary.border.width > 0.0, "{scheme:?} primary edge width");
        for (name, style) in [
            ("secondary", secondary_button(scheme)),
            ("danger", danger_button(scheme)),
        ] {
            let disabled = style(&theme, Status::Disabled);
            assert!(
                disabled.background.is_none()
                    || disabled.background == Some(Background::Color(Color::TRANSPARENT)),
                "{scheme:?} {name} fill {:?}",
                disabled.background
            );
            assert_eq!(disabled.border.color, t.line, "{scheme:?} {name} edge");
            assert!(disabled.border.width > 0.0, "{scheme:?} {name} edge width");
        }
    }
}

// --- the toggle stays visible --------------------------------------------------

fn luminance(color: Color) -> f32 {
    let channel = |c: f32| {
        if c <= 0.039_28 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * channel(color.r) + 0.7152 * channel(color.g) + 0.0722 * channel(color.b)
}

fn contrast(a: Color, b: Color) -> f32 {
    let (la, lb) = (luminance(a), luminance(b));
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

fn solid(background: iced::Background) -> Color {
    match background {
        iced::Background::Color(color) => color,
        iced::Background::Gradient(_) => panic!("a toggle part must be a solid colour"),
    }
}

#[test]
fn contrast_matches_the_wcag_reference_pair() {
    // black on white is 21:1 by the WCAG definition
    assert!((contrast(Color::BLACK, Color::WHITE) - 21.0).abs() < 0.01);
}

#[test]
fn an_off_toggle_knob_clears_3_to_1_against_its_track() {
    use iced::widget::toggler::Status;
    for scheme in [Scheme::Dark, Scheme::Light] {
        let theme = scheme.theme();
        for (name, style) in [("toggler", toggler(scheme)), ("dim", dim_toggler(scheme))] {
            let off = style(&theme, Status::Active { is_toggled: false });
            let ratio = contrast(solid(off.foreground), solid(off.background));
            assert!(ratio >= 3.0, "{scheme:?} {name} knob vs track {ratio:.2}");
        }
    }
}

#[test]
fn an_unknown_sliders_handle_clears_3_to_1_against_its_rail_and_the_card() {
    use iced::widget::slider::Status;
    for scheme in [Scheme::Dark, Scheme::Light] {
        let theme = scheme.theme();
        let style = dim_slider(scheme)(&theme, Status::Active);
        let handle = solid(style.handle.background);
        let rail = solid(style.rail.backgrounds.1);
        let card = scheme.tokens().surface_1;
        let on_rail = contrast(handle, rail);
        let on_card = contrast(handle, card);
        assert!(on_rail >= 3.0, "{scheme:?} handle vs rail {on_rail:.2}");
        assert!(on_card >= 3.0, "{scheme:?} handle vs card {on_card:.2}");
    }
}

// --- fonts, radii, control height ----------------------------------------------

#[test]
fn fonts_are_requested_by_family_name() {
    assert_eq!(UI_FONT.family, Family::Name("Onest"));
    assert_eq!(MONO_FONT.family, Family::Name("JetBrains Mono"));
}

#[test]
fn the_radius_ladder_and_control_height_match_the_tokens() {
    assert!((RADIUS_CONTROL - 10.0).abs() < f32::EPSILON);
    assert!((RADIUS_CARD - 14.0).abs() < f32::EPSILON);
    assert!((RADIUS_MODAL - 18.0).abs() < f32::EPSILON);
    assert!((CONTROL_HEIGHT - 36.0).abs() < f32::EPSILON);
}

// --- the window's app id -------------------------------------------------------

#[cfg(target_os = "linux")]
#[test]
fn the_window_names_its_app_id_gctl() {
    assert_eq!(
        crate::gui::view::window_settings()
            .platform_specific
            .application_id,
        "gctl"
    );
}
