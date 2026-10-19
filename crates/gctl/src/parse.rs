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

/// Parses a non-negative integer with no leading `+` and no sign at all.
fn digits<T: std::str::FromStr>(s: &str) -> Result<T, ()> {
    if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) {
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

/// `key:[mod+]...<Code>`, `mouse:<fn>`, `media:<fn>`, `dpi:<fn>`, `keyboard:<fn>` or `none`;
/// modifiers are `ctrl`, `shift`, `alt`, `meta`, `meta-right`.
pub fn binding(s: &str) -> Result<Binding, ParseError> {
    if s == "none" {
        return Ok(Binding::None);
    }
    let (family, rest) = s
        .split_once(':')
        .ok_or_else(|| err(format!("invalid binding {s:?}")))?;
    match family {
        "key" => {
            let mut parts = rest.split('+');
            let key_name = parts
                .next_back()
                .ok_or_else(|| err(format!("invalid binding {s:?}")))?;
            let key = Key::from_name(key_name).map_err(|e| err(e.to_string()))?;
            let mut modifiers = Modifiers::NONE;
            for name in parts {
                modifiers = modifiers
                    | match name {
                        "ctrl" => Modifiers::CTRL_LEFT,
                        "shift" => Modifiers::SHIFT_LEFT,
                        "alt" => Modifiers::ALT_LEFT,
                        "meta" => Modifiers::META_LEFT,
                        "meta-right" => Modifiers::META_RIGHT,
                        _ => return Err(err(format!("unknown modifier {name:?}"))),
                    };
            }
            Ok(Binding::Key { key, modifiers })
        }
        "mouse" => Ok(Binding::Mouse(match rest {
            "left" => MouseFn::Left,
            "right" => MouseFn::Right,
            "scroll" => MouseFn::Scroll,
            "back" => MouseFn::Back,
            "forward" => MouseFn::Forward,
            "scroll-up" => MouseFn::ScrollUp,
            "scroll-down" => MouseFn::ScrollDown,
            "profile-cycle-up" => MouseFn::ProfileCycleUp,
            "profile-cycle-down" => MouseFn::ProfileCycleDown,
            "battery-status" => MouseFn::BatteryStatus,
            _ => return Err(err(format!("unknown mouse function {rest:?}"))),
        })),
        "media" => Ok(Binding::Media(match rest {
            "player" => MediaFn::Player,
            "play-pause" => MediaFn::PlayPause,
            "next" => MediaFn::Next,
            "previous" => MediaFn::Previous,
            "stop" => MediaFn::Stop,
            "mute" => MediaFn::Mute,
            "volume-up" => MediaFn::VolumeUp,
            "volume-down" => MediaFn::VolumeDown,
            _ => return Err(err(format!("unknown media function {rest:?}"))),
        })),
        "dpi" => Ok(Binding::Dpi(match rest {
            "stage-up" => DpiFn::StageUp,
            "stage-down" => DpiFn::StageDown,
            "cycle-up" => DpiFn::CycleUp,
            "cycle-down" => DpiFn::CycleDown,
            _ => return Err(err(format!("unknown DPI function {rest:?}"))),
        })),
        "keyboard" => Ok(Binding::Keyboard(match rest {
            "profile-cycle-up" => KeyboardFn::ProfileCycleUp,
            "profile-cycle-down" => KeyboardFn::ProfileCycleDown,
            "layer-cycle-up" => KeyboardFn::LayerCycleUp,
            "layer-cycle-down" => KeyboardFn::LayerCycleDown,
            _ => return Err(err(format!("unknown keyboard function {rest:?}"))),
        })),
        _ => Err(err(format!("unknown binding family {family:?}"))),
    }
}

/// Inverse of [`binding`].
pub fn format_binding(binding: Binding) -> String {
    match binding {
        Binding::Key { key, modifiers } => {
            let mut out = String::from("key:");
            for (bit, name) in [
                (Modifiers::CTRL_LEFT, "ctrl"),
                (Modifiers::SHIFT_LEFT, "shift"),
                (Modifiers::ALT_LEFT, "alt"),
                (Modifiers::META_LEFT, "meta"),
                (Modifiers::META_RIGHT, "meta-right"),
            ] {
                if modifiers.bits() & bit.bits() != 0 {
                    out.push_str(name);
                    out.push('+');
                }
            }
            out.push_str(key.name());
            out
        }
        Binding::Mouse(function) => format!("mouse:{}", mouse_fn_name(function)),
        Binding::Media(function) => format!("media:{}", media_fn_name(function)),
        Binding::Dpi(function) => format!("dpi:{}", dpi_fn_name(function)),
        Binding::Keyboard(function) => format!("keyboard:{}", keyboard_fn_name(function)),
        Binding::None => "none".to_owned(),
    }
}

fn mouse_fn_name(function: MouseFn) -> &'static str {
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

fn media_fn_name(function: MediaFn) -> &'static str {
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

fn dpi_fn_name(function: DpiFn) -> &'static str {
    match function {
        DpiFn::StageUp => "stage-up",
        DpiFn::StageDown => "stage-down",
        DpiFn::CycleUp => "cycle-up",
        DpiFn::CycleDown => "cycle-down",
    }
}

fn keyboard_fn_name(function: KeyboardFn) -> &'static str {
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

    let speed = |rate: Option<Rate>| rate.unwrap_or(Rate::DEFAULT);
    match name {
        "off" => {
            require_none(name, &colors, &rate)?;
            Ok(Effect::Off)
        }
        "glorious" => {
            require_no_colors(name, &colors)?;
            Ok(Effect::Glorious { rate: speed(rate) })
        }
        "cycle" => {
            require_no_colors(name, &colors)?;
            Ok(Effect::Cycle { rate: speed(rate) })
        }
        "pulse" => {
            let effect = Effect::Pulse {
                rate: speed(rate),
                colors,
            };
            effect.check().map_err(|e| err(e.to_string()))?;
            Ok(effect)
        }
        "solid" => {
            require_no_rate(name, &rate)?;
            match colors.as_slice() {
                [color] => Ok(Effect::Solid { color: *color }),
                _ => Err(err("solid takes exactly one colour")),
            }
        }
        "pulse-one" => match colors.as_slice() {
            [color] => Ok(Effect::PulseOne {
                rate: speed(rate),
                color: *color,
            }),
            _ => Err(err("pulse-one takes exactly one colour")),
        },
        "tail" => {
            require_no_colors(name, &colors)?;
            Ok(Effect::Tail { rate: speed(rate) })
        }
        "rave" => {
            let effect = Effect::Rave {
                rate: speed(rate),
                colors,
            };
            effect.check().map_err(|e| err(e.to_string()))?;
            Ok(effect)
        }
        "wave" => {
            require_no_colors(name, &colors)?;
            Ok(Effect::Wave { rate: speed(rate) })
        }
        _ => Err(err(format!("unknown effect {name:?}"))),
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
