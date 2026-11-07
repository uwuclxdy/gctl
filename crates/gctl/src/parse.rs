//! Value grammar shared by the CLI, the state memory and the export file.

use std::borrow::Cow;

use gctl_proto::{
    Battery, Binding, Brightness, Button, Debounce, DeviceState, Dpi, DpiFn, DpiStage, DpiStages,
    Effect, Firmware, Key, KeyboardFn, LiftOff, MediaFn, Modifiers, MouseFn, PollingRate, Profile,
    Rate, Rgb, ScrollDirection, SleepDelay,
};

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct ParseError(pub String);

fn err(message: impl Into<String>) -> ParseError {
    ParseError(message.into())
}

/// Escapes every character `char::escape_debug` escapes except `"`, `'` and
/// `\`, which stay verbatim, so a value serde already quotes is not escaped
/// twice while a byte from a file (a control, bidi override or line separator)
/// still never reaches the terminal.
pub fn escape_for_terminal(input: &str) -> Cow<'_, str> {
    if !input.chars().any(needs_escaping) {
        return Cow::Borrowed(input);
    }
    let mut out = String::with_capacity(input.len());
    for c in input.chars() {
        if needs_escaping(c) {
            out.extend(c.escape_debug());
        } else {
            out.push(c);
        }
    }
    Cow::Owned(out)
}

fn needs_escaping(c: char) -> bool {
    !matches!(c, '"' | '\'' | '\\') && c.escape_debug().nth(1).is_some()
}

/// Whether `value` is a non-empty whole decimal number with no sign and no
/// separators. The GUI's sleep minutes/seconds fields refuse anything else.
pub fn is_whole_number(value: &str) -> bool {
    !value.is_empty() && value.bytes().all(|b| b.is_ascii_digit())
}

/// Parses a non-negative integer with no leading `+` and no sign at all.
fn digits<T: std::str::FromStr>(s: &str) -> Result<T, ()> {
    if !is_whole_number(s) {
        return Err(());
    }
    s.parse().map_err(|_| ())
}

/// `2m`, `90s`, `1m30s` or `never`.
pub fn sleep(s: &str) -> Result<SleepDelay, ParseError> {
    if s == "never" {
        return Ok(SleepDelay::Never);
    }
    let (minutes, seconds) = match s.as_bytes().last() {
        Some(b'm') => (parse_number(&s[..s.len() - 1], s)?, 0),
        Some(b's') => {
            let body = &s[..s.len() - 1];
            match body.find('m') {
                Some(at) => (
                    parse_number(&body[..at], s)?,
                    parse_number(&body[at + 1..], s)?,
                ),
                None => (0, parse_number(body, s)?),
            }
        }
        _ => return Err(err(format!("invalid sleep delay {s:?}"))),
    };
    let total = minutes
        .checked_mul(60)
        .and_then(|m| m.checked_add(seconds))
        .ok_or_else(|| err(format!("sleep delay {s:?} is too long")))?;
    if total == 0 {
        return Err(err(format!("sleep delay {s:?} is zero; use `never`")));
    }
    if total > 0xFFFE {
        return Err(err(format!("sleep delay {s:?} exceeds 65534 seconds")));
    }
    Ok(SleepDelay::Seconds(total as u16))
}

fn parse_number(part: &str, full: &str) -> Result<u64, ParseError> {
    digits(part).map_err(|_| err(format!("invalid sleep delay {full:?}")))
}

/// Inverse of [`sleep`]: `Nm` when the seconds part is 0, else `MmSs` when minutes > 0,
/// else `Ns`: 120 -> `2m`, 90 -> `1m30s`, 45 -> `45s`, and `never`.
pub fn format_sleep(delay: SleepDelay) -> String {
    match delay {
        SleepDelay::Never => "never".to_owned(),
        SleepDelay::Seconds(total) => {
            let (minutes, seconds) = (total / 60, total % 60);
            if seconds == 0 {
                format!("{minutes}m")
            } else if minutes > 0 {
                format!("{minutes}m{seconds}s")
            } else {
                format!("{seconds}s")
            }
        }
    }
}

/// Hz: `125`, `250`, `500` or `1000`.
pub fn polling_rate(s: &str) -> Result<PollingRate, ParseError> {
    match s {
        "125" => Ok(PollingRate::Hz125),
        "250" => Ok(PollingRate::Hz250),
        "500" => Ok(PollingRate::Hz500),
        "1000" => Ok(PollingRate::Hz1000),
        _ => Err(err(format!("invalid polling rate {s:?}"))),
    }
}

/// Hz as a number: `125`, `250`, `500` or `1000`.
pub fn polling_rate_hz(hz: u16) -> Result<PollingRate, ParseError> {
    match hz {
        125 => Ok(PollingRate::Hz125),
        250 => Ok(PollingRate::Hz250),
        500 => Ok(PollingRate::Hz500),
        1000 => Ok(PollingRate::Hz1000),
        other => Err(err(format!("unknown polling rate {other}"))),
    }
}

/// Inverse of [`polling_rate`]: the Hz number.
pub fn format_polling_rate(rate: PollingRate) -> String {
    rate.hz().to_string()
}

/// The wire/TOML number for a polling rate.
pub fn polling_rate_value(rate: PollingRate) -> u16 {
    rate.hz()
}

/// `1` or `2` mm.
pub fn lift_off(s: &str) -> Result<LiftOff, ParseError> {
    match s {
        "1" => Ok(LiftOff::Mm1),
        "2" => Ok(LiftOff::Mm2),
        other => Err(err(format!("lift-off must be 1 or 2, got {other:?}"))),
    }
}

/// Lift-off distance as a number: `1` or `2` mm.
pub fn lift_off_number(mm: u8) -> Result<LiftOff, ParseError> {
    match mm {
        1 => Ok(LiftOff::Mm1),
        2 => Ok(LiftOff::Mm2),
        other => Err(err(format!("unknown lift-off distance {other}"))),
    }
}

/// The wire/TOML number for a lift-off distance.
pub fn lift_off_value(lift_off: LiftOff) -> u8 {
    match lift_off {
        LiftOff::Mm1 => 1,
        LiftOff::Mm2 => 2,
    }
}

/// Inverse of [`lift_off`]: `1` or `2`.
pub fn format_lift_off(lift_off: LiftOff) -> &'static str {
    match lift_off {
        LiftOff::Mm1 => "1",
        LiftOff::Mm2 => "2",
    }
}

/// `left`, `right`, `scroll`, `back`, `forward`, `scroll-up`, `scroll-down`, `dpi`.
pub fn button(s: &str) -> Result<Button, ParseError> {
    match s {
        "left" => Ok(Button::Left),
        "right" => Ok(Button::Right),
        "scroll" => Ok(Button::Scroll),
        "back" => Ok(Button::Back),
        "forward" => Ok(Button::Forward),
        "scroll-up" => Ok(Button::ScrollUp),
        "scroll-down" => Ok(Button::ScrollDown),
        "dpi" => Ok(Button::Dpi),
        _ => Err(err(format!("unknown button {s:?}"))),
    }
}

/// The canonical button name, inverse of [`button`].
pub fn format_button(button: Button) -> &'static str {
    match button {
        Button::Left => "left",
        Button::Right => "right",
        Button::Scroll => "scroll",
        Button::Back => "back",
        Button::Forward => "forward",
        Button::ScrollUp => "scroll-up",
        Button::ScrollDown => "scroll-down",
        Button::Dpi => "dpi",
    }
}

/// `invert` or `default`.
pub fn scroll(s: &str) -> Result<ScrollDirection, ParseError> {
    match s {
        "invert" => Ok(ScrollDirection::Invert),
        "default" => Ok(ScrollDirection::Default),
        _ => Err(err(format!("invalid scroll direction {s:?}"))),
    }
}

/// Inverse of [`scroll`].
pub fn format_scroll(direction: ScrollDirection) -> &'static str {
    match direction {
        ScrollDirection::Default => "default",
        ScrollDirection::Invert => "invert",
    }
}

/// The modifier bits and their names, in the CLI's order. The one table
/// [`binding`], [`format_binding`] and the GUI's key toggles all read, so a
/// modifier name cannot drift between them.
pub const MODIFIERS: [(Modifiers, &str); 5] = [
    (Modifiers::CTRL_LEFT, "ctrl"),
    (Modifiers::SHIFT_LEFT, "shift"),
    (Modifiers::ALT_LEFT, "alt"),
    (Modifiers::META_LEFT, "meta"),
    (Modifiers::META_RIGHT, "meta-right"),
];

/// The modifier names a key bind takes, in the CLI's order.
pub const MODIFIER_NAMES: [&str; 5] = modifier_names(MODIFIERS);

const fn modifier_names(table: [(Modifiers, &str); 5]) -> [&str; 5] {
    [table[0].1, table[1].1, table[2].1, table[3].1, table[4].1]
}

/// The modifier bit a modifier name sets, the one mapping for `binding` and the
/// GUI's modifier toggles.
pub fn modifier_bits(name: &str) -> Option<Modifiers> {
    MODIFIERS
        .iter()
        .find(|(_, modifier)| *modifier == name)
        .map(|(bits, _)| *bits)
}

/// A binding family, paired with its token in [`BIND_FAMILIES`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BindCategory {
    None,
    Mouse,
    Media,
    Dpi,
    Keyboard,
    Key,
}

/// The token a family category is spelled with. The match is exhaustive, so a
/// new category is a compile error, never an out-of-range index.
pub const fn family_token(category: BindCategory) -> &'static str {
    match category {
        BindCategory::None => "none",
        BindCategory::Mouse => "mouse",
        BindCategory::Media => "media",
        BindCategory::Dpi => "dpi",
        BindCategory::Keyboard => "keyboard",
        BindCategory::Key => "key",
    }
}

/// The one table pairing each family token with its category, read by the
/// parser, the formatter and the window. The tokens come from [`family_token`],
/// the single source, so a category cannot drift from its token.
pub const BIND_FAMILIES: [(BindCategory, &str); 6] = [
    (BindCategory::None, family_token(BindCategory::None)),
    (BindCategory::Mouse, family_token(BindCategory::Mouse)),
    (BindCategory::Media, family_token(BindCategory::Media)),
    (BindCategory::Dpi, family_token(BindCategory::Dpi)),
    (BindCategory::Keyboard, family_token(BindCategory::Keyboard)),
    (BindCategory::Key, family_token(BindCategory::Key)),
];

/// The family tokens, in the CLI's order, for a picker's option list.
pub const BIND_FAMILY_TOKENS: [&str; 6] = family_tokens(BIND_FAMILIES);

const fn family_tokens(table: [(BindCategory, &str); 6]) -> [&str; 6] {
    [
        table[0].1, table[1].1, table[2].1, table[3].1, table[4].1, table[5].1,
    ]
}

/// The category a family token names, `None` for an unknown token.
pub fn bind_category(token: &str) -> Option<BindCategory> {
    BIND_FAMILIES
        .iter()
        .find(|(_, family)| *family == token)
        .map(|(category, _)| *category)
}

/// `key:[mod+]...<Code>`, `mouse:<fn>`, `media:<fn>`, `dpi:<fn>`, `keyboard:<fn>` or `none`;
/// modifiers are `ctrl`, `shift`, `alt`, `meta`, `meta-right`.
pub fn binding(s: &str) -> Result<Binding, ParseError> {
    if s == "none" {
        return Ok(Binding::None);
    }
    let (family, rest) = s
        .split_once(':')
        .ok_or_else(|| err(format!("invalid binding {s:?}")))?;
    match bind_category(family) {
        Some(BindCategory::Key) => {
            let mut parts = rest.split('+');
            let key_name = parts
                .next_back()
                .ok_or_else(|| err(format!("invalid binding {s:?}")))?;
            let key = Key::from_name(key_name).map_err(|e| err(e.to_string()))?;
            let mut modifiers = Modifiers::NONE;
            for name in parts {
                modifiers = modifiers
                    | modifier_bits(name)
                        .ok_or_else(|| err(format!("unknown modifier {name:?}")))?;
            }
            Ok(Binding::Key { key, modifiers })
        }
        Some(BindCategory::Mouse) => {
            let function = fn_of(rest, &MOUSE_FN_ALL, mouse_fn_name)
                .ok_or_else(|| err(format!("unknown mouse function {rest:?}")))?;
            Ok(Binding::Mouse(function))
        }
        Some(BindCategory::Media) => {
            let function = fn_of(rest, &MEDIA_FN_ALL, media_fn_name)
                .ok_or_else(|| err(format!("unknown media function {rest:?}")))?;
            Ok(Binding::Media(function))
        }
        Some(BindCategory::Dpi) => {
            let function = fn_of(rest, &DPI_FN_ALL, dpi_fn_name)
                .ok_or_else(|| err(format!("unknown DPI function {rest:?}")))?;
            Ok(Binding::Dpi(function))
        }
        Some(BindCategory::Keyboard) => {
            let function = fn_of(rest, &KEYBOARD_FN_ALL, keyboard_fn_name)
                .ok_or_else(|| err(format!("unknown keyboard function {rest:?}")))?;
            Ok(Binding::Keyboard(function))
        }
        Some(BindCategory::None) | None => Err(err(format!("unknown binding family {family:?}"))),
    }
}

/// The function in `all` whose formatter output is `name`.
fn fn_of<T: Copy>(name: &str, all: &[T], format: fn(T) -> &'static str) -> Option<T> {
    all.iter()
        .copied()
        .find(|function| format(*function) == name)
}

/// Inverse of [`binding`]. The family tokens read [`family_token`], the one
/// table the parser, the formatter and the window share.
pub fn format_binding(binding: Binding) -> String {
    match binding {
        Binding::Key { key, modifiers } => {
            let mut out = format!("{}:", family_token(BindCategory::Key));
            for (bits, name) in MODIFIERS {
                if modifiers.bits() & bits.bits() != 0 {
                    out.push_str(name);
                    out.push('+');
                }
            }
            out.push_str(key.name());
            out
        }
        Binding::Mouse(function) => {
            format!(
                "{}:{}",
                family_token(BindCategory::Mouse),
                mouse_fn_name(function)
            )
        }
        Binding::Media(function) => {
            format!(
                "{}:{}",
                family_token(BindCategory::Media),
                media_fn_name(function)
            )
        }
        Binding::Dpi(function) => {
            format!(
                "{}:{}",
                family_token(BindCategory::Dpi),
                dpi_fn_name(function)
            )
        }
        Binding::Keyboard(function) => {
            format!(
                "{}:{}",
                family_token(BindCategory::Keyboard),
                keyboard_fn_name(function)
            )
        }
        Binding::None => family_token(BindCategory::None).to_owned(),
    }
}

/// Maps a variant list through its name formatter, so a new enum variant
/// compiles into the formatter and cannot miss the picker's name list.
const fn mouse_names(all: [MouseFn; 10]) -> [&'static str; 10] {
    let mut names = [""; 10];
    let mut i = 0;
    while i < 10 {
        names[i] = mouse_fn_name(all[i]);
        i += 1;
    }
    names
}

const fn media_names(all: [MediaFn; 8]) -> [&'static str; 8] {
    let mut names = [""; 8];
    let mut i = 0;
    while i < 8 {
        names[i] = media_fn_name(all[i]);
        i += 1;
    }
    names
}

const fn dpi_names(all: [DpiFn; 4]) -> [&'static str; 4] {
    let mut names = [""; 4];
    let mut i = 0;
    while i < 4 {
        names[i] = dpi_fn_name(all[i]);
        i += 1;
    }
    names
}

const fn keyboard_names(all: [KeyboardFn; 4]) -> [&'static str; 4] {
    let mut names = [""; 4];
    let mut i = 0;
    while i < 4 {
        names[i] = keyboard_fn_name(all[i]);
        i += 1;
    }
    names
}

/// Every mouse function, in the CLI's order.
pub const MOUSE_FN_ALL: [MouseFn; 10] = [
    MouseFn::Left,
    MouseFn::Right,
    MouseFn::Scroll,
    MouseFn::Back,
    MouseFn::Forward,
    MouseFn::ScrollUp,
    MouseFn::ScrollDown,
    MouseFn::ProfileCycleUp,
    MouseFn::ProfileCycleDown,
    MouseFn::BatteryStatus,
];

/// Every media function, in the CLI's order.
pub const MEDIA_FN_ALL: [MediaFn; 8] = [
    MediaFn::Player,
    MediaFn::PlayPause,
    MediaFn::Next,
    MediaFn::Previous,
    MediaFn::Stop,
    MediaFn::Mute,
    MediaFn::VolumeUp,
    MediaFn::VolumeDown,
];

/// Every DPI function, in the CLI's order.
pub const DPI_FN_ALL: [DpiFn; 4] = [
    DpiFn::StageUp,
    DpiFn::StageDown,
    DpiFn::CycleUp,
    DpiFn::CycleDown,
];

/// Every keyboard function, in the CLI's order.
pub const KEYBOARD_FN_ALL: [KeyboardFn; 4] = [
    KeyboardFn::ProfileCycleUp,
    KeyboardFn::ProfileCycleDown,
    KeyboardFn::LayerCycleUp,
    KeyboardFn::LayerCycleDown,
];

/// Every mouse function name, in the CLI's order, derived from [`MOUSE_FN_ALL`].
pub const MOUSE_FN_NAMES: [&str; 10] = mouse_names(MOUSE_FN_ALL);

/// Every media function name, in the CLI's order.
pub const MEDIA_FN_NAMES: [&str; 8] = media_names(MEDIA_FN_ALL);

/// Every DPI function name, in the CLI's order.
pub const DPI_FN_NAMES: [&str; 4] = dpi_names(DPI_FN_ALL);

/// Every keyboard function name, in the CLI's order.
pub const KEYBOARD_FN_NAMES: [&str; 4] = keyboard_names(KEYBOARD_FN_ALL);

const fn mouse_fn_name(function: MouseFn) -> &'static str {
    match function {
        MouseFn::Left => "left",
        MouseFn::Right => "right",
        MouseFn::Scroll => "scroll",
        MouseFn::Back => "back",
        MouseFn::Forward => "forward",
        MouseFn::ScrollUp => "scroll-up",
        MouseFn::ScrollDown => "scroll-down",
        MouseFn::ProfileCycleUp => "profile-cycle-up",
        MouseFn::ProfileCycleDown => "profile-cycle-down",
        MouseFn::BatteryStatus => "battery-status",
    }
}

const fn media_fn_name(function: MediaFn) -> &'static str {
    match function {
        MediaFn::Player => "player",
        MediaFn::PlayPause => "play-pause",
        MediaFn::Next => "next",
        MediaFn::Previous => "previous",
        MediaFn::Stop => "stop",
        MediaFn::Mute => "mute",
        MediaFn::VolumeUp => "volume-up",
        MediaFn::VolumeDown => "volume-down",
    }
}

const fn dpi_fn_name(function: DpiFn) -> &'static str {
    match function {
        DpiFn::StageUp => "stage-up",
        DpiFn::StageDown => "stage-down",
        DpiFn::CycleUp => "cycle-up",
        DpiFn::CycleDown => "cycle-down",
    }
}

const fn keyboard_fn_name(function: KeyboardFn) -> &'static str {
    match function {
        KeyboardFn::ProfileCycleUp => "profile-cycle-up",
        KeyboardFn::ProfileCycleDown => "profile-cycle-down",
        KeyboardFn::LayerCycleUp => "layer-cycle-up",
        KeyboardFn::LayerCycleDown => "layer-cycle-down",
    }
}

/// A profile id: `1`, `2` or `3`, in its canonical single-digit form.
pub fn profile(value: &str) -> Result<Profile, ParseError> {
    if value.len() != 1 {
        return Err(err(format!("profile must be in 1..=3, got {value:?}")));
    }
    let id =
        digits::<u8>(value).map_err(|_| err(format!("profile must be in 1..=3, got {value:?}")))?;
    Profile::new(id).map_err(|e| err(e.to_string()))
}

/// A profile id as a number: `1`, `2` or `3`.
pub fn profile_number(id: u8) -> Result<Profile, ParseError> {
    Profile::new(id).map_err(|e| err(e.to_string()))
}

/// Inverse of [`profile`]: the profile id.
pub fn format_profile(profile: Profile) -> String {
    profile.get().to_string()
}

/// One DPI value, `100..=19000`.
pub fn dpi(value: &str) -> Result<Dpi, ParseError> {
    let dpi = digits::<u16>(value)
        .map_err(|_| err(format!("DPI must be in 100..=19000, got {value:?}")))?;
    Dpi::new(dpi).map_err(|e| err(e.to_string()))
}

/// A DPI stage list, 1-6 stages, from the CLI's text values.
pub fn dpi_stages<'a>(values: impl IntoIterator<Item = &'a str>) -> Result<DpiStages, ParseError> {
    let stages = values.into_iter().map(dpi).collect::<Result<Vec<_>, _>>()?;
    DpiStages::new(stages).map_err(|e| err(e.to_string()))
}

/// A DPI stage list, 1-6 stages, from the import file's numbers.
pub fn dpi_stages_numbers(values: &[u16]) -> Result<DpiStages, ParseError> {
    let stages = values
        .iter()
        .map(|value| Dpi::new(*value).map_err(|e| err(e.to_string())))
        .collect::<Result<Vec<_>, _>>()?;
    DpiStages::new(stages).map_err(|e| err(e.to_string()))
}

/// Inverse of [`dpi_stages`]: `400 800 1600`.
pub fn format_dpi_stages(stages: &DpiStages) -> String {
    stages
        .as_slice()
        .iter()
        .map(|stage| stage.get().to_string())
        .collect::<Vec<_>>()
        .join(" ")
}

/// The stage list as numbers, inverse of [`dpi_stages_numbers`].
pub fn dpi_stages_values(stages: &DpiStages) -> Vec<u16> {
    stages.as_slice().iter().map(|stage| stage.get()).collect()
}

/// The active DPI stage, `1..=6`.
pub fn dpi_stage(value: &str) -> Result<DpiStage, ParseError> {
    let stage = digits::<u8>(value)
        .map_err(|_| err(format!("dpi-stage must be in 1..=6, got {value:?}")))?;
    DpiStage::new(stage).map_err(|e| err(e.to_string()))
}

/// The active DPI stage as a number, `1..=6`.
pub fn dpi_stage_number(stage: u8) -> Result<DpiStage, ParseError> {
    DpiStage::new(stage).map_err(|e| err(e.to_string()))
}

/// Inverse of [`dpi_stage`]: the stage number.
pub fn format_dpi_stage(stage: DpiStage) -> String {
    stage.get().to_string()
}

/// The active DPI stage as a number, inverse of [`dpi_stage_number`].
pub fn dpi_stage_value(stage: DpiStage) -> u8 {
    stage.get()
}

/// One colour, six hex digits like `ff8800`.
pub fn color(value: &str) -> Result<Rgb, ParseError> {
    value
        .parse()
        .map_err(|e: gctl_proto::Error| err(e.to_string()))
}

/// A colour list, one per configured DPI stage: 1-6 colours.
pub fn colors<'a>(values: impl IntoIterator<Item = &'a str>) -> Result<Vec<Rgb>, ParseError> {
    let values: Vec<&str> = values.into_iter().collect();
    if values.is_empty() {
        return Err(err("dpi-colors takes at least one value"));
    }
    if values.len() > DpiStages::MAX {
        return Err(err(format!(
            "dpi-colors takes at most {} values",
            DpiStages::MAX
        )));
    }
    values.into_iter().map(color).collect()
}

/// Inverse of [`color`]: six hex digits.
pub fn format_color(color: Rgb) -> String {
    color.to_string()
}

/// Inverse of [`colors`]: `ff0000 00ff00`.
pub fn format_colors(colors: &[Rgb]) -> String {
    colors
        .iter()
        .map(|color| format_color(*color))
        .collect::<Vec<_>>()
        .join(" ")
}

/// `200 50`: wired then wireless LED brightness, each `0..=255`.
pub fn brightness(value: &str) -> Result<Brightness, ParseError> {
    let mut parts = value.split_whitespace();
    let wired = parts
        .next()
        .ok_or_else(|| err("brightness takes wired and wireless values"))?;
    let wireless = parts
        .next()
        .ok_or_else(|| err("brightness takes wired and wireless values"))?;
    if parts.next().is_some() {
        return Err(err("brightness takes wired and wireless values"));
    }
    brightness_parts(wired, wireless)
}

/// Wired then wireless LED brightness, each `0..=255`, as two separate values.
pub fn brightness_parts(wired: &str, wireless: &str) -> Result<Brightness, ParseError> {
    Ok(Brightness {
        wired: brightness_number(wired)?,
        wireless: brightness_number(wireless)?,
    })
}

fn brightness_number(value: &str) -> Result<u8, ParseError> {
    digits::<u8>(value).map_err(|_| err(format!("brightness must be 0..=255, got {value:?}")))
}

/// Inverse of [`brightness`]: `200 50`.
pub fn format_brightness(brightness: Brightness) -> String {
    format!("{} {}", brightness.wired, brightness.wireless)
}

/// LED brightness per link as numbers, inverse of [`brightness`].
pub fn brightness_values(brightness: Brightness) -> (u8, u8) {
    (brightness.wired, brightness.wireless)
}

/// LED brightness from its two numbers, inverse of [`brightness_values`]. The
/// numbers are `u8`, so every value is already in range.
pub fn brightness_numbers(wired: u8, wireless: u8) -> Brightness {
    Brightness { wired, wireless }
}

/// Debounce time in milliseconds, `0..=16`.
pub fn debounce(value: &str) -> Result<Debounce, ParseError> {
    let ms =
        digits::<u8>(value).map_err(|_| err(format!("debounce must be 0..=16, got {value:?}")))?;
    Debounce::new(ms).map_err(|e| err(e.to_string()))
}

/// Debounce time as a number, `0..=16`.
pub fn debounce_number(ms: u8) -> Result<Debounce, ParseError> {
    Debounce::new(ms).map_err(|e| err(e.to_string()))
}

/// Inverse of [`debounce`]: the millisecond value.
pub fn format_debounce(debounce: Debounce) -> String {
    debounce.get().to_string()
}

/// Debounce time as a number, inverse of [`debounce_number`].
pub fn debounce_value(debounce: Debounce) -> u8 {
    debounce.get()
}

/// `<name> [<colour>...] [rate=<0-100>]`, e.g. `wave rate=40`, `solid 00ff00`.
pub fn effect(s: &str) -> Result<Effect, ParseError> {
    let mut tokens = s.split_whitespace();
    let name = tokens.next().ok_or_else(|| err("missing effect name"))?;

    let mut colors: Vec<Rgb> = Vec::new();
    let mut rate: Option<Rate> = None;
    for token in tokens {
        if let Some(value) = token.strip_prefix("rate=") {
            if rate.is_some() {
                return Err(err(format!("effect {s:?} sets the rate twice")));
            }
            let number = digits::<u8>(value).map_err(|_| err(format!("invalid rate {value:?}")))?;
            rate = Some(Rate::new(number).map_err(|e| err(e.to_string()))?);
        } else if rate.is_some() {
            return Err(err(format!("colours must come before the rate: {s:?}")));
        } else {
            colors.push(color(token)?);
        }
    }

    let Some((_, make)) = EFFECTS.iter().find(|(token, _)| *token == name).copied() else {
        return Err(err(format!("unknown effect {name:?}")));
    };
    make(name, colors, rate)
}

/// One effect name and the builder that validates the arguments it takes. The
/// parser looks names up here and the window's picker offers [`EFFECT_NAMES`]
/// built from the same table, so neither can name an effect the other rejects.
type EffectMake = fn(&str, Vec<Rgb>, Option<Rate>) -> Result<Effect, ParseError>;

const EFFECTS: [(&str, EffectMake); 9] = [
    ("off", effect_off),
    ("glorious", effect_glorious),
    ("cycle", effect_cycle),
    ("pulse", effect_pulse),
    ("solid", effect_solid),
    ("pulse-one", effect_pulse_one),
    ("tail", effect_tail),
    ("rave", effect_rave),
    ("wave", effect_wave),
];

/// The effect names the CLI accepts, in the order the window offers them.
pub const EFFECT_NAMES: [&str; 9] = effect_names(&EFFECTS);

const fn effect_names(table: &[(&'static str, EffectMake); 9]) -> [&'static str; 9] {
    let mut names = [""; 9];
    let mut index = 0;
    while index < 9 {
        names[index] = table[index].0;
        index += 1;
    }
    names
}

fn speed(rate: Option<Rate>) -> Rate {
    rate.unwrap_or(Rate::DEFAULT)
}

fn effect_off(name: &str, colors: Vec<Rgb>, rate: Option<Rate>) -> Result<Effect, ParseError> {
    require_none(name, &colors, &rate)?;
    Ok(Effect::Off)
}

fn effect_glorious(name: &str, colors: Vec<Rgb>, rate: Option<Rate>) -> Result<Effect, ParseError> {
    require_no_colors(name, &colors)?;
    Ok(Effect::Glorious { rate: speed(rate) })
}

fn effect_cycle(name: &str, colors: Vec<Rgb>, rate: Option<Rate>) -> Result<Effect, ParseError> {
    require_no_colors(name, &colors)?;
    Ok(Effect::Cycle { rate: speed(rate) })
}

fn effect_pulse(_name: &str, colors: Vec<Rgb>, rate: Option<Rate>) -> Result<Effect, ParseError> {
    let effect = Effect::Pulse {
        rate: speed(rate),
        colors,
    };
    effect.check().map_err(|e| err(e.to_string()))?;
    Ok(effect)
}

fn effect_solid(name: &str, colors: Vec<Rgb>, rate: Option<Rate>) -> Result<Effect, ParseError> {
    require_no_rate(name, &rate)?;
    match colors.as_slice() {
        [color] => Ok(Effect::Solid { color: *color }),
        _ => Err(err("solid takes exactly one colour")),
    }
}

fn effect_pulse_one(
    _name: &str,
    colors: Vec<Rgb>,
    rate: Option<Rate>,
) -> Result<Effect, ParseError> {
    match colors.as_slice() {
        [color] => Ok(Effect::PulseOne {
            rate: speed(rate),
            color: *color,
        }),
        _ => Err(err("pulse-one takes exactly one colour")),
    }
}

fn effect_tail(name: &str, colors: Vec<Rgb>, rate: Option<Rate>) -> Result<Effect, ParseError> {
    require_no_colors(name, &colors)?;
    Ok(Effect::Tail { rate: speed(rate) })
}

fn effect_rave(_name: &str, colors: Vec<Rgb>, rate: Option<Rate>) -> Result<Effect, ParseError> {
    let effect = Effect::Rave {
        rate: speed(rate),
        colors,
    };
    effect.check().map_err(|e| err(e.to_string()))?;
    Ok(effect)
}

fn effect_wave(name: &str, colors: Vec<Rgb>, rate: Option<Rate>) -> Result<Effect, ParseError> {
    require_no_colors(name, &colors)?;
    Ok(Effect::Wave { rate: speed(rate) })
}

/// The window's label for an effect name: the CLI token title-cased, its
/// hyphens as spaces (`pulse-one` -> `Pulse one`).
pub fn effect_label(name: &str) -> String {
    let mut chars = name.chars();
    match chars.next() {
        Some(first) => {
            let mut out = first.to_uppercase().collect::<String>();
            out.push_str(chars.as_str());
            out.replace('-', " ")
        }
        None => String::new(),
    }
}

/// Inverse of [`effect`].
pub fn format_effect(effect: &Effect) -> String {
    match effect {
        Effect::Off => "off".to_owned(),
        Effect::Glorious { rate } => format!("glorious rate={}", rate.get()),
        Effect::Cycle { rate } => format!("cycle rate={}", rate.get()),
        Effect::Pulse { rate, colors } => {
            format!("pulse {} rate={}", format_colors(colors), rate.get())
        }
        Effect::Solid { color } => format!("solid {color}"),
        Effect::PulseOne { rate, color } => {
            format!("pulse-one {color} rate={}", rate.get())
        }
        Effect::Tail { rate } => format!("tail rate={}", rate.get()),
        Effect::Rave { rate, colors } => {
            format!("rave {} rate={}", format_colors(colors), rate.get())
        }
        Effect::Wave { rate } => format!("wave rate={}", rate.get()),
    }
}

fn require_no_colors(name: &str, colors: &[Rgb]) -> Result<(), ParseError> {
    if colors.is_empty() {
        Ok(())
    } else {
        Err(err(format!("{name} takes no colour")))
    }
}

fn require_no_rate(name: &str, rate: &Option<Rate>) -> Result<(), ParseError> {
    if rate.is_none() {
        Ok(())
    } else {
        Err(err(format!("{name} takes no rate")))
    }
}

fn require_none(name: &str, colors: &[Rgb], rate: &Option<Rate>) -> Result<(), ParseError> {
    require_no_colors(name, colors)?;
    require_no_rate(name, rate)
}

/// The battery as the CLI prints it: the percent, or a state word.
pub fn format_battery(battery: Battery) -> String {
    match battery.state {
        DeviceState::Awake => match battery.percent {
            Some(percent) => percent.to_string(),
            None => "unknown".to_owned(),
        },
        DeviceState::Asleep => "asleep".to_owned(),
        DeviceState::WakingUp => "waking up".to_owned(),
        DeviceState::Unknown(_) | DeviceState::Invalid => "unknown".to_owned(),
    }
}

/// The firmware version as the CLI prints it, e.g. `0.3.8.0`.
pub fn format_firmware(firmware: Firmware) -> String {
    firmware.to_string()
}

#[cfg(test)]
#[path = "../tests/unit/parse_effects.rs"]
mod effect_tests;
