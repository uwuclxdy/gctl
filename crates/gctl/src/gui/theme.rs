//! The window's colour schemes, fonts, shape tokens and widget styles.

use iced::theme::palette::Palette;
use iced::theme::{Mode, Theme};
use iced::widget::{
    button, checkbox as checkbox_widget, container, pick_list as pick_list_widget, rule,
    scrollable as scrollable_widget, slider as slider_widget, text_input as text_input_widget,
    toggler as toggler_widget,
};
use iced::{Background, Border, Color, Font, Shadow, Vector};

#[cfg(test)]
#[path = "../../tests/unit/gui_theme.rs"]
mod tests;

/// One colour scheme. The window starts dark and follows the desktop's mode
/// once the runtime answers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Scheme {
    Dark,
    Light,
}

impl Scheme {
    /// The scheme a desktop [`Mode`] asks for: only `Light` is light.
    pub(crate) fn from_mode(mode: Mode) -> Scheme {
        match mode {
            Mode::Light => Scheme::Light,
            Mode::Dark | Mode::None => Scheme::Dark,
        }
    }

    pub(crate) fn tokens(self) -> &'static Tokens {
        match self {
            Scheme::Dark => &DARK,
            Scheme::Light => &LIGHT,
        }
    }

    /// The iced [`Theme`] carrying this scheme's base colours.
    pub(crate) fn theme(self) -> Theme {
        let tokens = self.tokens();
        Theme::custom(
            match self {
                Scheme::Dark => "gctl-dark",
                Scheme::Light => "gctl-light",
            },
            Palette {
                background: tokens.bg,
                text: tokens.text,
                primary: tokens.accent,
                success: tokens.success,
                warning: tokens.warning,
                danger: tokens.danger,
            },
        )
    }
}

/// The window's named colours, one per design token, per scheme.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Tokens {
    pub(crate) well: Color,
    pub(crate) bg: Color,
    pub(crate) surface_1: Color,
    pub(crate) surface_2: Color,
    pub(crate) surface_3: Color,
    pub(crate) line: Color,
    pub(crate) line_soft: Color,
    pub(crate) text: Color,
    pub(crate) text_2: Color,
    pub(crate) text_3: Color,
    pub(crate) text_4: Color,
    pub(crate) accent: Color,
    pub(crate) accent_hover: Color,
    pub(crate) accent_press: Color,
    pub(crate) accent_text: Color,
    pub(crate) accent_quiet: Color,
    pub(crate) accent_line: Color,
    pub(crate) on_accent: Color,
    pub(crate) success: Color,
    pub(crate) warning: Color,
    pub(crate) danger: Color,
    pub(crate) danger_text: Color,
    pub(crate) on_danger: Color,
    /// The modal scrim, the design's `--backdrop`.
    pub(crate) backdrop: Color,
    /// The float shadow, the widest `--elev-float` layer.
    pub(crate) shadow: Color,
}

const fn rgb(hex: u32) -> Color {
    Color::from_rgb8((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
}

const fn rgba(hex: u32, alpha: f32) -> Color {
    Color::from_rgba8((hex >> 16) as u8, (hex >> 8) as u8, hex as u8, alpha)
}

/// The dark scheme, the window's default until the desktop reports otherwise.
static DARK: Tokens = Tokens {
    // --well
    well: rgb(0x171923),
    // --bg
    bg: rgb(0x20222E),
    // --surface-1
    surface_1: rgb(0x2B2D3A),
    // --surface-2
    surface_2: rgb(0x363847),
    // --surface-3
    surface_3: rgb(0x434555),
    // --line
    line: rgb(0x363847),
    // --line-soft
    line_soft: rgb(0x2E2F3C),
    // --text
    text: rgb(0xDADCF2),
    // --text-2
    text_2: rgb(0xB9BCD1),
    // --text-3
    text_3: rgb(0xA0A3B8),
    // --text-4
    text_4: rgb(0x9497AB),
    // --accent
    accent: rgb(0x68BBEF),
    // --accent-hover
    accent_hover: rgb(0x77CFFF),
    // --accent-press
    accent_press: rgb(0x59A8D8),
    // --accent-text
    accent_text: rgb(0x77CFFF),
    // --accent-quiet
    accent_quiet: rgba(0x68BBEF, 0.16),
    // --accent-line
    accent_line: rgba(0x68BBEF, 0.38),
    // --on-accent
    on_accent: rgb(0x191A24),
    // --success
    success: rgb(0x7CD591),
    // --warning
    warning: rgb(0xE6C873),
    // --danger
    danger: rgb(0xEF7D88),
    // --danger-text
    danger_text: rgb(0xFF99A4),
    // --on-danger
    on_danger: rgb(0x231718),
    // --backdrop
    backdrop: rgba(0x0B0C16, 0.66),
    // --elev-float (widest layer)
    shadow: rgba(0x020207, 0.66),
};

/// The light scheme, following a light desktop.
static LIGHT: Tokens = Tokens {
    // --well
    well: rgb(0xE4E6EC),
    // --bg
    bg: rgb(0xEFF0F4),
    // --surface-1
    surface_1: rgb(0xFDFDFF),
    // --surface-2
    surface_2: rgb(0xE8E9EF),
    // --surface-3
    surface_3: rgb(0xDDDFE6),
    // --line
    line: rgb(0xD8DAE3),
    // --line-soft
    line_soft: rgb(0xE2E4EB),
    // --text
    text: rgb(0x2B2D38),
    // --text-2
    text_2: rgb(0x4A4C58),
    // --text-3
    text_3: rgb(0x60636E),
    // --text-4
    text_4: rgb(0x676A74),
    // --accent
    accent: rgb(0x007AB9),
    // --accent-hover
    accent_hover: rgb(0x0068A3),
    // --accent-press
    accent_press: rgb(0x00598E),
    // --accent-text
    accent_text: rgb(0x006BA4),
    // --accent-quiet
    accent_quiet: rgba(0x007AB9, 0.12),
    // --accent-line
    accent_line: rgba(0x007AB9, 0.34),
    // --on-accent
    on_accent: rgb(0xFCFEFF),
    // --success
    success: rgb(0x00792F),
    // --warning
    warning: rgb(0x805E00),
    // --danger
    danger: rgb(0xBD1F44),
    // --danger-text
    danger_text: rgb(0xA80034),
    // --on-danger
    on_danger: rgb(0xFFFDFD),
    // --backdrop
    backdrop: rgba(0x525561, 0.32),
    // --elev-float (widest layer)
    shadow: rgba(0x6C7184, 0.21),
};

/// The UI font, requested by family name; iced falls back to its default sans
/// where Onest is missing.
pub(crate) const UI_FONT: Font = Font::with_name("Onest");

/// The mono font, for numeric readouts only.
pub(crate) const MONO_FONT: Font = Font::with_name("JetBrains Mono");

pub(crate) const RADIUS_CONTROL: f32 = 10.0;
pub(crate) const RADIUS_CARD: f32 = 14.0;
pub(crate) const RADIUS_MODAL: f32 = 18.0;
pub(crate) const CONTROL_HEIGHT: f32 = 36.0;

/// A translucent tint of `color`, for the status tags and the warning badge.
fn tint(color: Color, alpha: f32) -> Color {
    let [r, g, b, _] = color.into_rgba8();
    Color::from_rgba8(r, g, b, alpha)
}

fn control_radius() -> Border {
    Border {
        radius: RADIUS_CONTROL.into(),
        ..Border::default()
    }
}

// --- buttons ------------------------------------------------------------------

/// The one filled button: accent fill, on-accent label.
pub(crate) fn filled_button(scheme: Scheme) -> button::StyleFn<'static, Theme> {
    Box::new(move |_theme, status| {
        let t = scheme.tokens();
        let (background, text_color) = match status {
            button::Status::Active => (Some(t.accent), t.on_accent),
            button::Status::Hovered => (Some(t.accent_hover), t.on_accent),
            button::Status::Pressed => (Some(t.accent_press), t.on_accent),
            button::Status::Disabled => (Some(t.surface_2), t.text_4),
        };
        button::Style {
            background: background.map(Background::Color),
            text_color,
            border: control_radius(),
            ..button::Style::default()
        }
    })
}

/// An outlined primary: accent-quiet fill, accent-text label, accent-line edge.
pub(crate) fn primary_button(scheme: Scheme) -> button::StyleFn<'static, Theme> {
    Box::new(move |_theme, status| {
        let t = scheme.tokens();
        let (background, text_color, border_color) = match status {
            button::Status::Active => (Some(t.accent_quiet), t.accent_text, t.accent_line),
            button::Status::Hovered => (Some(t.surface_2), t.accent_text, t.accent_line),
            button::Status::Pressed => (Some(t.surface_3), t.accent_text, t.accent_line),
            button::Status::Disabled => (Some(t.surface_2), t.text_4, t.surface_3),
        };
        button::Style {
            background: background.map(Background::Color),
            text_color,
            border: Border {
                radius: RADIUS_CONTROL.into(),
                width: 1.0,
                color: border_color,
            },
            ..button::Style::default()
        }
    })
}

/// A secondary button: transparent with a surface-3 edge.
pub(crate) fn secondary_button(scheme: Scheme) -> button::StyleFn<'static, Theme> {
    Box::new(move |_theme, status| {
        let t = scheme.tokens();
        let (background, text_color, border_color) = match status {
            button::Status::Active => (None, t.text, t.surface_3),
            button::Status::Hovered => (Some(t.surface_2), t.text, t.surface_3),
            button::Status::Pressed => (Some(t.surface_3), t.text, t.surface_3),
            button::Status::Disabled => (None, t.text_4, t.line),
        };
        button::Style {
            background: background.map(Background::Color),
            text_color,
            border: Border {
                radius: RADIUS_CONTROL.into(),
                width: 1.0,
                color: border_color,
            },
            ..button::Style::default()
        }
    })
}

/// An outlined danger button: danger edge, danger label stepping to danger-text
/// on hover and press.
pub(crate) fn danger_button(scheme: Scheme) -> button::StyleFn<'static, Theme> {
    Box::new(move |_theme, status| {
        let t = scheme.tokens();
        let (background, text_color, border_color) = match status {
            button::Status::Active => (None, t.danger, t.danger),
            button::Status::Hovered => (Some(tint(t.danger, 0.14)), t.danger_text, t.danger),
            button::Status::Pressed => (Some(tint(t.danger, 0.22)), t.danger_text, t.danger),
            button::Status::Disabled => (None, t.text_4, t.line),
        };
        button::Style {
            background: background.map(Background::Color),
            text_color,
            border: Border {
                radius: RADIUS_CONTROL.into(),
                width: 1.0,
                color: border_color,
            },
            ..button::Style::default()
        }
    })
}

/// A quiet (ghost) button: no fill or edge at rest, a surface on hover.
pub(crate) fn quiet_button(scheme: Scheme) -> button::StyleFn<'static, Theme> {
    Box::new(move |_theme, status| {
        let t = scheme.tokens();
        let (background, text_color) = match status {
            button::Status::Active => (None, t.text_2),
            button::Status::Hovered => (Some(t.surface_2), t.text_2),
            button::Status::Pressed => (Some(t.surface_3), t.text_2),
            button::Status::Disabled => (None, t.text_4),
        };
        button::Style {
            background: background.map(Background::Color),
            text_color,
            border: control_radius(),
            ..button::Style::default()
        }
    })
}

/// The edited profile's digit: it keeps its selected look even while picks are
/// blocked, so the sidebar still marks which profile is being edited.
pub(crate) fn pick_selected(scheme: Scheme) -> button::StyleFn<'static, Theme> {
    Box::new(move |_theme, status| {
        let t = scheme.tokens();
        let background = match status {
            button::Status::Pressed => t.surface_3,
            button::Status::Hovered => t.surface_2,
            button::Status::Active | button::Status::Disabled => t.accent_quiet,
        };
        button::Style {
            background: Some(Background::Color(background)),
            text_color: t.accent_text,
            border: Border {
                radius: RADIUS_CONTROL.into(),
                width: 1.0,
                color: t.accent_line,
            },
            ..button::Style::default()
        }
    })
}

// --- inputs and pickers -------------------------------------------------------

/// A text input sunk to the well, its focus edge the accent.
pub(crate) fn text_input(scheme: Scheme) -> text_input_widget::StyleFn<'static, Theme> {
    Box::new(move |_theme, status| {
        let t = scheme.tokens();
        let (border, value) = match status {
            text_input_widget::Status::Active | text_input_widget::Status::Hovered => (
                Border {
                    radius: RADIUS_CONTROL.into(),
                    width: 1.0,
                    color: t.line,
                },
                t.text,
            ),
            text_input_widget::Status::Focused { .. } => (
                Border {
                    radius: RADIUS_CONTROL.into(),
                    width: 2.0,
                    color: t.accent,
                },
                t.text,
            ),
            text_input_widget::Status::Disabled => (
                Border {
                    radius: RADIUS_CONTROL.into(),
                    width: 1.0,
                    color: t.line_soft,
                },
                t.text_4,
            ),
        };
        text_input_widget::Style {
            background: Background::Color(t.well),
            border,
            icon: t.text_3,
            placeholder: t.text_4,
            value,
            selection: t.accent_quiet,
        }
    })
}

/// A pick list sunk to the well.
pub(crate) fn pick_list(scheme: Scheme) -> pick_list_widget::StyleFn<'static, Theme> {
    Box::new(move |_theme, status| {
        let t = scheme.tokens();
        let border = match status {
            pick_list_widget::Status::Active | pick_list_widget::Status::Hovered => Border {
                radius: RADIUS_CONTROL.into(),
                width: 1.0,
                color: t.line,
            },
            pick_list_widget::Status::Opened { .. } => Border {
                radius: RADIUS_CONTROL.into(),
                width: 2.0,
                color: t.accent,
            },
        };
        pick_list_widget::Style {
            text_color: t.text,
            placeholder_color: t.text_4,
            handle_color: t.text_3,
            background: Background::Color(t.well),
            border,
        }
    })
}

/// The pick list's menu: surface-1, a hairline, an accent-tinted selection.
pub(crate) fn pick_menu(scheme: Scheme) -> impl Fn(&Theme) -> iced::overlay::menu::Style + 'static {
    move |_theme| {
        let t = scheme.tokens();
        iced::overlay::menu::Style {
            background: Background::Color(t.surface_1),
            border: Border {
                radius: RADIUS_CONTROL.into(),
                width: 1.0,
                color: t.line,
            },
            text_color: t.text,
            selected_text_color: t.accent_text,
            selected_background: Background::Color(t.accent_quiet),
            shadow: Shadow {
                color: t.shadow,
                offset: Vector::new(0.0, 4.0),
                blur_radius: 18.0,
            },
        }
    }
}

// --- sliders, toggles, checkboxes --------------------------------------------

/// A live slider: accent fill over a well rail.
pub(crate) fn slider(scheme: Scheme) -> slider_widget::StyleFn<'static, Theme> {
    Box::new(move |_theme, status| {
        let t = scheme.tokens();
        let handle_color = match status {
            slider_widget::Status::Active => t.accent,
            slider_widget::Status::Hovered => t.accent_hover,
            slider_widget::Status::Dragged => t.accent_press,
        };
        slider_widget::Style {
            rail: slider_widget::Rail {
                backgrounds: (Background::Color(t.accent), Background::Color(t.well)),
                width: 4.0,
                border: Border {
                    radius: 999.0.into(),
                    ..Border::default()
                },
            },
            handle: slider_widget::Handle {
                shape: slider_widget::HandleShape::Circle { radius: 7.0 },
                background: Background::Color(handle_color),
                border_width: 0.0,
                border_color: Color::TRANSPARENT,
            },
        }
    })
}

/// A dimmed slider for a write-only value with nothing remembered: the
/// disabled toggle's tones, clearing 3:1 against its rail and the card.
pub(crate) fn dim_slider(scheme: Scheme) -> slider_widget::StyleFn<'static, Theme> {
    Box::new(move |_theme, _status| {
        let t = scheme.tokens();
        slider_widget::Style {
            rail: slider_widget::Rail {
                backgrounds: (
                    Background::Color(t.surface_2),
                    Background::Color(t.surface_2),
                ),
                width: 4.0,
                border: Border {
                    radius: 999.0.into(),
                    ..Border::default()
                },
            },
            handle: slider_widget::Handle {
                shape: slider_widget::HandleShape::Circle { radius: 7.0 },
                background: Background::Color(t.text_4),
                border_width: 0.0,
                border_color: Color::TRANSPARENT,
            },
        }
    })
}

/// A toggle: accent track when on, a surface-3 track when off, a text knob.
pub(crate) fn toggler(scheme: Scheme) -> toggler_widget::StyleFn<'static, Theme> {
    Box::new(move |_theme, status| {
        let t = scheme.tokens();
        let (track, knob) = match status {
            toggler_widget::Status::Active { is_toggled }
            | toggler_widget::Status::Hovered { is_toggled } => {
                (if is_toggled { t.accent } else { t.surface_3 }, t.text)
            }
            toggler_widget::Status::Disabled { is_toggled } => (
                if is_toggled {
                    t.accent_quiet
                } else {
                    t.surface_2
                },
                t.text_4,
            ),
        };
        toggler_widget::Style {
            background: Background::Color(track),
            background_border_width: 0.0,
            background_border_color: Color::TRANSPARENT,
            foreground: Background::Color(knob),
            foreground_border_width: 0.0,
            foreground_border_color: Color::TRANSPARENT,
            text_color: None,
            border_radius: None,
            padding_ratio: 0.2,
        }
    })
}

/// A dimmed toggle for an unknown value: surface-2 track, text-4 knob.
pub(crate) fn dim_toggler(scheme: Scheme) -> toggler_widget::StyleFn<'static, Theme> {
    Box::new(move |_theme, status| {
        let t = scheme.tokens();
        let (track, knob) = match status {
            toggler_widget::Status::Active { is_toggled }
            | toggler_widget::Status::Hovered { is_toggled } => (
                if is_toggled {
                    t.accent_quiet
                } else {
                    t.surface_2
                },
                t.text_4,
            ),
            toggler_widget::Status::Disabled { is_toggled } => {
                (if is_toggled { t.surface_2 } else { t.well }, t.text_4)
            }
        };
        toggler_widget::Style {
            background: Background::Color(track),
            background_border_width: 0.0,
            background_border_color: Color::TRANSPARENT,
            foreground: Background::Color(knob),
            foreground_border_width: 0.0,
            foreground_border_color: Color::TRANSPARENT,
            text_color: None,
            border_radius: None,
            padding_ratio: 0.2,
        }
    })
}

/// A checkbox: accent fill when checked, a well box when not.
pub(crate) fn checkbox(scheme: Scheme) -> checkbox_widget::StyleFn<'static, Theme> {
    Box::new(move |_theme, status| {
        let t = scheme.tokens();
        let (background, icon) = match status {
            checkbox_widget::Status::Active { is_checked }
            | checkbox_widget::Status::Hovered { is_checked } => (
                if is_checked { t.accent } else { t.well },
                if is_checked { t.on_accent } else { t.text_4 },
            ),
            checkbox_widget::Status::Disabled { is_checked } => (
                if is_checked {
                    t.accent_quiet
                } else {
                    t.surface_2
                },
                t.text_4,
            ),
        };
        checkbox_widget::Style {
            background: Background::Color(background),
            icon_color: icon,
            border: Border {
                radius: 4.0.into(),
                width: 1.0,
                color: t.line,
            },
            text_color: Some(t.text_2),
        }
    })
}

// --- containers, rules, scrollers ---------------------------------------------

/// A card: surface-1 with a 1px line drawn as the border (iced has no shadow
/// spread, so the card line is a border).
pub(crate) fn card(scheme: Scheme) -> impl Fn(&Theme) -> container::Style + 'static {
    move |_theme| {
        let t = scheme.tokens();
        container::Style {
            background: Some(Background::Color(t.surface_1)),
            border: Border {
                radius: RADIUS_CARD.into(),
                width: 1.0,
                color: t.line,
            },
            ..container::Style::default()
        }
    }
}

/// The sidebar's flush card: square on the window's left, top and bottom edges.
pub(crate) fn sidebar(scheme: Scheme) -> impl Fn(&Theme) -> container::Style + 'static {
    move |_theme| {
        let t = scheme.tokens();
        container::Style {
            background: Some(Background::Color(t.surface_1)),
            border: Border {
                radius: iced::border::Radius {
                    top_left: 0.0,
                    top_right: 0.0,
                    bottom_right: 0.0,
                    bottom_left: 0.0,
                },
                width: 1.0,
                color: t.line,
            },
            ..container::Style::default()
        }
    }
}

/// A neutral mark tag: text-3 on a quiet surface tint, pill radius.
pub(crate) fn tag(scheme: Scheme) -> impl Fn(&Theme) -> container::Style + 'static {
    move |_theme| {
        let t = scheme.tokens();
        container::Style {
            background: Some(Background::Color(t.surface_2)),
            text_color: Some(t.text_3),
            border: Border {
                radius: 999.0.into(),
                ..Border::default()
            },
            ..container::Style::default()
        }
    }
}

/// A warning-tone tag, for the `Untested` badge.
pub(crate) fn tag_warning(scheme: Scheme) -> impl Fn(&Theme) -> container::Style + 'static {
    move |_theme| {
        let t = scheme.tokens();
        container::Style {
            background: Some(Background::Color(tint(t.warning, 0.14))),
            text_color: Some(t.warning),
            border: Border {
                radius: 999.0.into(),
                ..Border::default()
            },
            ..container::Style::default()
        }
    }
}

/// The error banner: danger text on a danger tint, square on every window edge
/// it touches.
pub(crate) fn banner(scheme: Scheme) -> impl Fn(&Theme) -> container::Style + 'static {
    move |_theme| {
        let t = scheme.tokens();
        container::Style {
            background: Some(Background::Color(tint(t.danger, 0.14))),
            text_color: Some(t.danger_text),
            border: Border {
                radius: iced::border::Radius {
                    top_left: 0.0,
                    top_right: 0.0,
                    bottom_right: 0.0,
                    bottom_left: 0.0,
                },
                width: 1.0,
                color: t.line,
            },
            ..container::Style::default()
        }
    }
}

/// The modal's card: surface-1, 18 radius, one hue-tinted float shadow.
pub(crate) fn modal(scheme: Scheme) -> impl Fn(&Theme) -> container::Style + 'static {
    move |_theme| {
        let t = scheme.tokens();
        container::Style {
            background: Some(Background::Color(t.surface_1)),
            border: Border {
                radius: RADIUS_MODAL.into(),
                ..Border::default()
            },
            shadow: Shadow {
                color: t.shadow,
                offset: Vector::new(0.0, 14.0),
                blur_radius: 22.0,
            },
            ..container::Style::default()
        }
    }
}

/// The modal's scrim: the design's backdrop token.
pub(crate) fn scrim(scheme: Scheme) -> impl Fn(&Theme) -> container::Style + 'static {
    move |_theme| {
        let t = scheme.tokens();
        container::Style {
            background: Some(Background::Color(t.backdrop)),
            ..container::Style::default()
        }
    }
}

/// A divider drawn in `line`.
pub(crate) fn rule(scheme: Scheme) -> impl Fn(&Theme) -> rule::Style + 'static {
    move |_theme| {
        let t = scheme.tokens();
        rule::Style {
            color: t.line,
            radius: 0.0.into(),
            fill_mode: rule::FillMode::Full,
            snap: true,
        }
    }
}

/// The page's scrollable: a transparent container, a quiet rail.
pub(crate) fn scrollable(scheme: Scheme) -> scrollable_widget::StyleFn<'static, Theme> {
    Box::new(move |_theme, status| {
        let t = scheme.tokens();
        let (dragged, hovered) = match status {
            scrollable_widget::Status::Active { .. } => (false, false),
            scrollable_widget::Status::Hovered {
                is_horizontal_scrollbar_hovered,
                is_vertical_scrollbar_hovered,
                ..
            } => (
                false,
                is_horizontal_scrollbar_hovered || is_vertical_scrollbar_hovered,
            ),
            scrollable_widget::Status::Dragged {
                is_horizontal_scrollbar_dragged,
                is_vertical_scrollbar_dragged,
                ..
            } => (
                is_horizontal_scrollbar_dragged || is_vertical_scrollbar_dragged,
                false,
            ),
        };
        let scroller = Background::Color(if dragged || hovered {
            t.surface_3
        } else {
            t.surface_2
        });
        let rail = scrollable_widget::Rail {
            background: None,
            border: Border {
                radius: 4.0.into(),
                ..Border::default()
            },
            scroller: scrollable_widget::Scroller {
                background: scroller,
                border: Border {
                    radius: 4.0.into(),
                    ..Border::default()
                },
            },
        };
        scrollable_widget::Style {
            container: container::Style::default(),
            vertical_rail: rail,
            horizontal_rail: rail,
            gap: None,
            auto_scroll: scrollable_widget::AutoScroll {
                background: Background::Color(t.surface_1),
                border: Border {
                    radius: 999.0.into(),
                    width: 1.0,
                    color: t.line,
                },
                shadow: Shadow {
                    color: t.shadow,
                    offset: Vector::ZERO,
                    blur_radius: 8.0,
                },
                icon: t.text_2,
            },
        }
    })
}
