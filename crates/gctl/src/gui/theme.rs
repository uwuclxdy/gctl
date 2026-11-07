//! The window's colour schemes, fonts, shape tokens and widget styles.

use iced::theme::palette::Palette;
use iced::theme::{Mode, Theme};
use iced::widget::{
    button, checkbox as checkbox_widget, container, rule, scrollable as scrollable_widget,
    slider as slider_widget, text_input as text_input_widget, toggler as toggler_widget,
};
use iced::{Background, Border, Color, Font, Shadow, Vector};

/// Onest, the UI face, as shipped in the binary.
static ONEST: &[u8] = include_bytes!("../../fonts/Onest-Variable.ttf");
/// JetBrains Mono, the numeric readouts' face.
static JETBRAINS_MONO: &[u8] = include_bytes!("../../fonts/JetBrainsMono-Variable.ttf");

/// The font files the window loads at start and every test simulator loads, so
/// no text falls back to the desktop's sans.
pub(crate) const FONTS: &[&[u8]] = &[ONEST, JETBRAINS_MONO];

#[cfg(test)]
#[path = "../../tests/unit/gui_theme.rs"]
mod tests;

#[cfg(test)]
#[path = "../../tests/unit/gui_surfaces.rs"]
mod surface_tests;

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

impl Tokens {
    /// The whole palette at a fraction of its opacity: iced has no subtree
    /// opacity, so a fading card threads one alpha into every colour it paints.
    pub(crate) fn scale_alpha(&self, alpha: f32) -> Tokens {
        let f = |color: Color| color.scale_alpha(alpha);
        Tokens {
            well: f(self.well),
            bg: f(self.bg),
            surface_1: f(self.surface_1),
            surface_2: f(self.surface_2),
            surface_3: f(self.surface_3),
            line: f(self.line),
            line_soft: f(self.line_soft),
            text: f(self.text),
            text_2: f(self.text_2),
            text_3: f(self.text_3),
            text_4: f(self.text_4),
            accent: f(self.accent),
            accent_hover: f(self.accent_hover),
            accent_press: f(self.accent_press),
            accent_text: f(self.accent_text),
            accent_quiet: f(self.accent_quiet),
            accent_line: f(self.accent_line),
            on_accent: f(self.on_accent),
            success: f(self.success),
            warning: f(self.warning),
            danger: f(self.danger),
            danger_text: f(self.danger_text),
            on_danger: f(self.on_danger),
            backdrop: f(self.backdrop),
            shadow: f(self.shadow),
        }
    }
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
    text_4: rgb(0x656771),
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
/// The tint multiplies the colour's own alpha, so a fading surface's tint fades
/// with it.
fn tint(color: Color, alpha: f32) -> Color {
    let mut tinted = color;
    tinted.a *= alpha;
    tinted
}

/// A control's hover surface at the fade `hover` (0 at rest, 1 hovered): the
/// button's own status snaps, so the paint takes the shell's eased alpha.
fn hovered(rest: Option<Color>, hover: Color, alpha: f32) -> Option<Color> {
    let alpha = alpha.clamp(0.0, 1.0);
    let rest = rest.unwrap_or(Color::TRANSPARENT);
    (rest.a > 0.0 || alpha > 0.0).then(|| mix(rest, hover, alpha))
}

/// `a` at `t = 0`, `b` at `t = 1`, straight through the channels: the one way
/// a colour moves between two tokens (a field rising into its open fill, an
/// edge settling to the accent).
pub(crate) fn mix(a: Color, b: Color, t: f32) -> Color {
    let t = t.clamp(0.0, 1.0);
    let lerp = |x: f32, y: f32| x + (y - x) * t;
    Color {
        r: lerp(a.r, b.r),
        g: lerp(a.g, b.g),
        b: lerp(a.b, b.b),
        a: lerp(a.a, b.a),
    }
}

fn control_radius() -> Border {
    Border {
        radius: RADIUS_CONTROL.into(),
        ..Border::default()
    }
}

/// The paint under a control, from its eased hover and its state. The control
/// widget draws it at its own clock, so no style is closed over a hover that
/// goes stale between rebuilds.
pub(crate) type Surface<'a> = Box<dyn Fn(f32, button::Status) -> button::Style + 'a>;

// --- buttons ------------------------------------------------------------------

/// The one filled button: accent fill, on-accent label.
#[cfg(test)]
pub(crate) fn filled_button(scheme: Scheme) -> button::StyleFn<'static, Theme> {
    filled_button_at(*scheme.tokens(), 0.0)
}

fn filled(t: Tokens, hover: f32, status: button::Status) -> button::Style {
    let (background, text_color) = match status {
        button::Status::Active | button::Status::Hovered => {
            (hovered(Some(t.accent), t.accent_hover, hover), t.on_accent)
        }
        button::Status::Pressed => (Some(t.accent_press), t.on_accent),
        button::Status::Disabled => (Some(t.surface_2), t.text_4),
    };
    button::Style {
        background: background.map(Background::Color),
        text_color,
        border: control_radius(),
        ..button::Style::default()
    }
}

pub(crate) fn filled_surface(t: Tokens) -> Surface<'static> {
    Box::new(move |hover, status| filled(t, hover, status))
}

#[cfg(test)]
pub(crate) fn filled_button_at(t: Tokens, hover: f32) -> button::StyleFn<'static, Theme> {
    Box::new(move |_theme, status| filled(t, hover, status))
}

/// An outlined primary: accent-quiet fill, accent-text label, accent-line edge.
#[cfg(test)]
pub(crate) fn primary_button(scheme: Scheme) -> button::StyleFn<'static, Theme> {
    primary_button_at(*scheme.tokens(), 0.0)
}

pub(crate) fn primary_surface(t: Tokens) -> Surface<'static> {
    Box::new(move |hover, status| primary(t, hover, status))
}

#[cfg(test)]
pub(crate) fn primary_button_at(t: Tokens, hover: f32) -> button::StyleFn<'static, Theme> {
    Box::new(move |_theme, status| primary(t, hover, status))
}

fn primary(t: Tokens, hover: f32, status: button::Status) -> button::Style {
    let (background, text_color, border_color) = match status {
        button::Status::Active | button::Status::Hovered => (
            hovered(Some(t.accent_quiet), t.surface_2, hover),
            t.accent_text,
            t.accent_line,
        ),
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
}

/// A secondary button: transparent with a surface-3 edge.
#[cfg(test)]
pub(crate) fn secondary_button(scheme: Scheme) -> button::StyleFn<'static, Theme> {
    secondary_button_at(*scheme.tokens(), 0.0)
}

pub(crate) fn secondary_surface(t: Tokens) -> Surface<'static> {
    Box::new(move |hover, status| secondary(t, hover, status))
}

#[cfg(test)]
pub(crate) fn secondary_button_at(t: Tokens, hover: f32) -> button::StyleFn<'static, Theme> {
    Box::new(move |_theme, status| secondary(t, hover, status))
}

fn secondary(t: Tokens, hover: f32, status: button::Status) -> button::Style {
    let (background, text_color, border_color) = match status {
        button::Status::Active | button::Status::Hovered => {
            (hovered(None, t.surface_2, hover), t.text, t.surface_3)
        }
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
}

/// The outside-change banner's Dismiss: a secondary button edged in the warning
/// tone, so its outline shows on the banner tint; never filled.
#[cfg(test)]
pub(crate) fn warning_button(scheme: Scheme) -> button::StyleFn<'static, Theme> {
    warning_button_at(*scheme.tokens(), 0.0)
}

pub(crate) fn warning_surface(t: Tokens) -> Surface<'static> {
    Box::new(move |hover, status| warning(t, hover, status))
}

#[cfg(test)]
pub(crate) fn warning_button_at(t: Tokens, hover: f32) -> button::StyleFn<'static, Theme> {
    Box::new(move |_theme, status| warning(t, hover, status))
}

fn warning(t: Tokens, hover: f32, status: button::Status) -> button::Style {
    let (background, text_color, border_color) = match status {
        button::Status::Active | button::Status::Hovered => {
            (hovered(None, t.surface_2, hover), t.text, t.warning)
        }
        button::Status::Pressed => (Some(t.surface_3), t.text, t.warning),
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
}

/// An outlined danger button: danger edge, danger label stepping to danger-text
/// on hover and press.
#[cfg(test)]
pub(crate) fn danger_button(scheme: Scheme) -> button::StyleFn<'static, Theme> {
    danger_button_at(*scheme.tokens(), 0.0)
}

pub(crate) fn danger_surface(t: Tokens) -> Surface<'static> {
    Box::new(move |hover, status| danger(t, hover, status))
}

#[cfg(test)]
pub(crate) fn danger_button_at(t: Tokens, hover: f32) -> button::StyleFn<'static, Theme> {
    Box::new(move |_theme, status| danger(t, hover, status))
}

fn danger(t: Tokens, hover: f32, status: button::Status) -> button::Style {
    let (background, text_color, border_color) = match status {
        button::Status::Active | button::Status::Hovered => (
            hovered(None, tint(t.danger, 0.14), hover),
            mix(t.danger, t.danger_text, hover),
            t.danger,
        ),
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
}

/// A quiet button: a hairline edge at rest, a surface on hover and press. It
/// draws a silhouette, so no action reads as bare text.
#[cfg(test)]
pub(crate) fn quiet_button(scheme: Scheme) -> button::StyleFn<'static, Theme> {
    quiet_button_at(*scheme.tokens(), 0.0)
}

pub(crate) fn quiet_surface(t: Tokens) -> Surface<'static> {
    Box::new(move |hover, status| quiet(t, hover, status))
}

#[cfg(test)]
pub(crate) fn quiet_button_at(t: Tokens, hover: f32) -> button::StyleFn<'static, Theme> {
    Box::new(move |_theme, status| quiet(t, hover, status))
}

fn quiet(t: Tokens, hover: f32, status: button::Status) -> button::Style {
    let (background, text_color, border_color) = match status {
        button::Status::Active | button::Status::Hovered => {
            (hovered(None, t.surface_2, hover), t.text_2, t.line)
        }
        button::Status::Pressed => (Some(t.surface_3), t.text, t.line),
        button::Status::Disabled => (None, t.text_4, t.line_soft),
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
}

pub(crate) fn nav_surface(t: Tokens, selected: f32) -> Surface<'static> {
    Box::new(move |hover, status| nav(t, selected, hover, status))
}
fn nav(t: Tokens, selected: f32, hover: f32, status: button::Status) -> button::Style {
    let (background, text_color) = match status {
        button::Status::Active | button::Status::Hovered => {
            let rest = t.accent_quiet.scale_alpha(selected.clamp(0.0, 1.0));
            let hover_fill = mix(t.surface_2, t.accent_quiet, selected.clamp(0.0, 1.0));
            (
                hovered(Some(rest), hover_fill, hover),
                mix(t.text, t.accent_text, selected),
            )
        }
        button::Status::Pressed => (Some(t.surface_3), t.text),
        button::Status::Disabled => (None, t.text_4),
    };
    button::Style {
        background: background.map(Background::Color),
        text_color,
        border: Border {
            radius: 8.0.into(),
            ..Border::default()
        },
        ..button::Style::default()
    }
}

// --- segmented control --------------------------------------------------------

/// The track that frames all three profile segments: a well fill inside one
/// hairline, so the unpicked digits read as part of one control.
pub(crate) fn segment_track(scheme: Scheme) -> impl Fn(&Theme) -> container::Style + 'static {
    segment_track_at(*scheme.tokens())
}

pub(crate) fn segment_track_at(t: Tokens) -> impl Fn(&Theme) -> container::Style + 'static {
    move |_theme| container::Style {
        background: Some(Background::Color(t.well)),
        border: Border {
            radius: 12.0.into(),
            width: 1.0,
            color: t.line,
        },
        ..container::Style::default()
    }
}

/// The selected segment's capsule, sliding under the digits.
pub(crate) fn segment_selected(scheme: Scheme) -> impl Fn(&Theme) -> container::Style + 'static {
    segment_selected_at(*scheme.tokens())
}

pub(crate) fn segment_selected_at(t: Tokens) -> impl Fn(&Theme) -> container::Style + 'static {
    move |_theme| container::Style {
        background: Some(Background::Color(t.accent_quiet)),
        border: Border {
            radius: 8.0.into(),
            width: 1.0,
            color: t.accent_line,
        },
        ..container::Style::default()
    }
}

pub(crate) fn segment_surface(t: Tokens, hovers: bool) -> Surface<'static> {
    Box::new(move |hover, _status| segment(t, if hovers { hover } else { 0.0 }))
}

fn segment(t: Tokens, hover: f32) -> button::Style {
    button::Style {
        background: hovered(None, t.surface_2, hover).map(Background::Color),
        text_color: t.text_2,
        border: Border {
            radius: 8.0.into(),
            ..Border::default()
        },
        ..button::Style::default()
    }
}

/// A picker's field: the well's fill, blending toward surface-2 under the
/// pointer. Its edge belongs to the dropdown, which alone knows whether the
/// list is open; the fill carries that edge's radius so no square corner shows
/// outside it.
pub(crate) fn field_surface(t: Tokens) -> Surface<'static> {
    Box::new(move |hover, _status| button::Style {
        background: Some(Background::Color(mix(
            t.well,
            t.surface_2,
            0.35 * hover.clamp(0.0, 1.0),
        ))),
        text_color: t.text,
        border: control_radius(),
        ..button::Style::default()
    })
}

/// The trigger's edge at `open` of the way from the quiet line to the accent.
pub(crate) fn trigger_edge(t: Tokens, open: f32) -> Border {
    Border {
        radius: RADIUS_CONTROL.into(),
        width: 1.0,
        color: mix(t.line, t.accent, open.clamp(0.0, 1.0)),
    }
}

// --- inputs and pickers -------------------------------------------------------

/// A text input sunk to the well, its focus edge the accent.
#[cfg(test)]
pub(crate) fn text_input(scheme: Scheme) -> text_input_widget::StyleFn<'static, Theme> {
    text_input_at(*scheme.tokens())
}

pub(crate) fn text_input_at(t: Tokens) -> text_input_widget::StyleFn<'static, Theme> {
    Box::new(move |_theme, status| {
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

// --- sliders, toggles, checkboxes --------------------------------------------

/// A live slider: accent fill over a well rail.
pub(crate) fn slider_at(t: Tokens) -> slider_widget::StyleFn<'static, Theme> {
    Box::new(move |_theme, status| {
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
#[cfg(test)]
pub(crate) fn dim_slider(scheme: Scheme) -> slider_widget::StyleFn<'static, Theme> {
    dim_slider_at(*scheme.tokens())
}

pub(crate) fn dim_slider_at(t: Tokens) -> slider_widget::StyleFn<'static, Theme> {
    Box::new(move |_theme, _status| slider_widget::Style {
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
    })
}

/// A toggle: accent track when on, a surface-3 track when off, a text knob.
#[cfg(test)]
pub(crate) fn toggler(scheme: Scheme) -> toggler_widget::StyleFn<'static, Theme> {
    toggler_at(*scheme.tokens())
}

pub(crate) fn toggler_at(t: Tokens) -> toggler_widget::StyleFn<'static, Theme> {
    Box::new(move |_theme, status| {
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
#[cfg(test)]
pub(crate) fn dim_toggler(scheme: Scheme) -> toggler_widget::StyleFn<'static, Theme> {
    dim_toggler_at(*scheme.tokens())
}

pub(crate) fn dim_toggler_at(t: Tokens) -> toggler_widget::StyleFn<'static, Theme> {
    Box::new(move |_theme, status| {
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
#[cfg(test)]
pub(crate) fn checkbox(scheme: Scheme) -> checkbox_widget::StyleFn<'static, Theme> {
    checkbox_at(*scheme.tokens())
}

pub(crate) fn checkbox_at(t: Tokens) -> checkbox_widget::StyleFn<'static, Theme> {
    Box::new(move |_theme, status| {
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
pub(crate) fn card_at(t: Tokens) -> impl Fn(&Theme) -> container::Style + 'static {
    move |_theme| container::Style {
        background: Some(Background::Color(t.surface_1)),
        border: Border {
            radius: RADIUS_CARD.into(),
            width: 1.0,
            color: t.line,
        },
        text_color: Some(t.text),
        ..container::Style::default()
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
pub(crate) fn tag_at(t: Tokens) -> impl Fn(&Theme) -> container::Style + 'static {
    move |_theme| container::Style {
        background: Some(Background::Color(t.surface_2)),
        text_color: Some(t.text_3),
        border: Border {
            radius: 999.0.into(),
            ..Border::default()
        },
        ..container::Style::default()
    }
}

/// The warning tint `tag_warning` and `banner_warning` share: the rendered
/// light banner text measures 4.53:1 on it.
const WARNING_TINT: f32 = 0.10;

/// A warning-tone tag, for the `Untested` badge.
pub(crate) fn tag_warning(scheme: Scheme) -> impl Fn(&Theme) -> container::Style + 'static {
    tag_warning_at(*scheme.tokens())
}

pub(crate) fn tag_warning_at(t: Tokens) -> impl Fn(&Theme) -> container::Style + 'static {
    move |_theme| container::Style {
        background: Some(Background::Color(tint(t.warning, WARNING_TINT))),
        text_color: Some(t.warning),
        border: Border {
            radius: 999.0.into(),
            ..Border::default()
        },
        ..container::Style::default()
    }
}

/// The banner shape both banners share: text on a tint, square on every window
/// edge it touches.
fn banner_style(
    background: Color,
    text: Color,
    line: Color,
) -> impl Fn(&Theme) -> container::Style + 'static {
    move |_theme| container::Style {
        background: Some(Background::Color(background)),
        text_color: Some(text),
        border: Border {
            radius: iced::border::Radius {
                top_left: 0.0,
                top_right: 0.0,
                bottom_right: 0.0,
                bottom_left: 0.0,
            },
            width: 1.0,
            color: line,
        },
        ..container::Style::default()
    }
}

/// The error banner: danger text on a danger tint.
pub(crate) fn banner_at(t: Tokens) -> impl Fn(&Theme) -> container::Style + 'static {
    banner_style(tint(t.danger, 0.14), t.danger_text, t.line)
}

/// The outside-change banner: warning text on the shared warning tint.
pub(crate) fn banner_warning_at(t: Tokens) -> impl Fn(&Theme) -> container::Style + 'static {
    banner_style(tint(t.warning, WARNING_TINT), t.warning, t.line)
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
    rule_at(*scheme.tokens())
}

pub(crate) fn rule_at(t: Tokens) -> impl Fn(&Theme) -> rule::Style + 'static {
    move |_theme| rule::Style {
        color: t.line,
        radius: 0.0.into(),
        fill_mode: rule::FillMode::Full,
        snap: true,
    }
}

/// The page's scrollable: a transparent container, a quiet rail.
pub(crate) fn scrollable_at(t: Tokens) -> scrollable_widget::StyleFn<'static, Theme> {
    Box::new(move |_theme, status| {
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
