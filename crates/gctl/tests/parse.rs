#![allow(clippy::unwrap_used, clippy::expect_used)]

//! The value grammar: every form `gctl set` accepts and `gctl get`, the memory
//! and the export file print.

use gctl::parse;
use gctl_proto::{
    Binding, Button, DpiFn, Effect, Key, KeyboardFn, MediaFn, Modifiers, MouseFn, PollingRate,
    Rate, Rgb, SleepDelay,
};

fn rgb(hex: &str) -> Rgb {
    hex.parse().unwrap()
}

fn rate(r: u8) -> Rate {
    Rate::new(r).unwrap()
}

fn key(name: &str) -> Key {
    Key::from_name(name).unwrap()
}

// ---------------------------------------------------------------- sleep

#[test]
fn sleep_parses_every_form() {
    assert_eq!(parse::sleep("2m").unwrap(), SleepDelay::Seconds(120));
    assert_eq!(parse::sleep("90s").unwrap(), SleepDelay::Seconds(90));
    assert_eq!(parse::sleep("1m30s").unwrap(), SleepDelay::Seconds(90));
    assert_eq!(parse::sleep("never").unwrap(), SleepDelay::Never);
}

#[test]
fn sleep_accepts_the_range_ends() {
    assert_eq!(parse::sleep("1s").unwrap(), SleepDelay::Seconds(1));
    assert_eq!(parse::sleep("65534s").unwrap(), SleepDelay::Seconds(65534));
    assert_eq!(
        parse::sleep("1092m14s").unwrap(),
        SleepDelay::Seconds(65534)
    );
}

#[test]
fn sleep_rejects_zero() {
    assert!(parse::sleep("0s").is_err(), "0s");
    assert!(parse::sleep("0m").is_err(), "0m");
    assert!(parse::sleep("0m0s").is_err(), "0m0s");
}

#[test]
fn sleep_rejects_values_past_65534_seconds() {
    assert!(parse::sleep("65535s").is_err(), "65535s");
    assert!(parse::sleep("1092m15s").is_err(), "1092m15s");
    assert!(parse::sleep("99999999m").is_err(), "overflowing minutes");
}

#[test]
fn sleep_rejects_malformed_text() {
    assert!(parse::sleep("").is_err(), "empty");
    assert!(parse::sleep("2").is_err(), "no unit");
    assert!(parse::sleep("30s1m").is_err(), "seconds before minutes");
    assert!(parse::sleep("m").is_err(), "unit only");
    assert!(parse::sleep("-5s").is_err(), "negative");
    assert!(parse::sleep("2h").is_err(), "hours");
}

#[test]
fn format_sleep_follows_the_text_form() {
    assert_eq!(parse::format_sleep(SleepDelay::Seconds(120)), "2m");
    assert_eq!(parse::format_sleep(SleepDelay::Seconds(90)), "1m30s");
    assert_eq!(parse::format_sleep(SleepDelay::Seconds(45)), "45s");
    assert_eq!(parse::format_sleep(SleepDelay::Seconds(60)), "1m");
    assert_eq!(parse::format_sleep(SleepDelay::Seconds(1)), "1s");
    assert_eq!(parse::format_sleep(SleepDelay::Seconds(65534)), "1092m14s");
    assert_eq!(parse::format_sleep(SleepDelay::Never), "never");
}

#[test]
fn sleep_round_trips() {
    for text in ["2m", "1m30s", "45s", "1m", "1s", "1092m14s", "never"] {
        let parsed = parse::sleep(text).unwrap();
        assert_eq!(parse::format_sleep(parsed), text, "{text}");
    }
}

// ---------------------------------------------------------------- polling rate

#[test]
fn polling_rate_takes_hertz() {
    assert_eq!(parse::polling_rate("125").unwrap(), PollingRate::Hz125);
    assert_eq!(parse::polling_rate("250").unwrap(), PollingRate::Hz250);
    assert_eq!(parse::polling_rate("500").unwrap(), PollingRate::Hz500);
    assert_eq!(parse::polling_rate("1000").unwrap(), PollingRate::Hz1000);
}

#[test]
fn polling_rate_rejects_other_values() {
    assert!(parse::polling_rate("300").is_err(), "300");
    assert!(parse::polling_rate("1000hz").is_err(), "unit suffix");
    assert!(parse::polling_rate("").is_err(), "empty");
}

// ---------------------------------------------------------------- buttons

#[test]
fn button_names() {
    let cases = [
        ("left", Button::Left),
        ("right", Button::Right),
        ("scroll", Button::Scroll),
        ("back", Button::Back),
        ("forward", Button::Forward),
        ("scroll-up", Button::ScrollUp),
        ("scroll-down", Button::ScrollDown),
        ("dpi", Button::Dpi),
    ];
    for (name, button) in cases {
        assert_eq!(parse::button(name).unwrap(), button, "{name}");
    }
}

#[test]
fn button_rejects_unknown_names() {
    assert!(parse::button("middle").is_err(), "middle");
    assert!(parse::button("Left").is_err(), "names are lower case");
}

// ---------------------------------------------------------------- binds

#[test]
fn binding_key_with_and_without_modifiers() {
    assert_eq!(
        parse::binding("key:KeyA").unwrap(),
        Binding::Key {
            key: key("KeyA"),
            modifiers: Modifiers::NONE
        }
    );
    assert_eq!(
        parse::binding("key:ctrl+KeyA").unwrap(),
        Binding::Key {
            key: key("KeyA"),
            modifiers: Modifiers::CTRL_LEFT
        }
    );
    assert_eq!(
        parse::binding("key:ctrl+shift+alt+meta+meta-right+F12").unwrap(),
        Binding::Key {
            key: key("F12"),
            modifiers: Modifiers::CTRL_LEFT
                | Modifiers::SHIFT_LEFT
                | Modifiers::ALT_LEFT
                | Modifiers::META_LEFT
                | Modifiers::META_RIGHT
        }
    );
}

#[test]
fn binding_function_families() {
    let cases = [
        ("mouse:left", Binding::Mouse(MouseFn::Left)),
        ("mouse:scroll", Binding::Mouse(MouseFn::Scroll)),
        ("mouse:scroll-down", Binding::Mouse(MouseFn::ScrollDown)),
        (
            "mouse:profile-cycle-up",
            Binding::Mouse(MouseFn::ProfileCycleUp),
        ),
        (
            "mouse:battery-status",
            Binding::Mouse(MouseFn::BatteryStatus),
        ),
        ("media:play-pause", Binding::Media(MediaFn::PlayPause)),
        ("media:volume-down", Binding::Media(MediaFn::VolumeDown)),
        ("dpi:stage-up", Binding::Dpi(DpiFn::StageUp)),
        ("dpi:cycle-down", Binding::Dpi(DpiFn::CycleDown)),
        (
            "keyboard:layer-cycle-up",
            Binding::Keyboard(KeyboardFn::LayerCycleUp),
        ),
        (
            "keyboard:profile-cycle-down",
            Binding::Keyboard(KeyboardFn::ProfileCycleDown),
        ),
        ("none", Binding::None),
    ];
    for (text, binding) in cases {
        assert_eq!(parse::binding(text).unwrap(), binding, "{text}");
    }
}

#[test]
fn binding_rejects_malformed_text() {
    let bad = [
        ("", "empty"),
        ("key:", "no key"),
        ("key:ctrl+", "modifier without key"),
        ("key:Foo", "unknown key"),
        ("key:ShiftRight", "outside mxw's key set"),
        ("key:alt-right+KeyA", "unknown modifier"),
        ("mouse:nope", "unknown mouse fn"),
        ("media:", "empty media fn"),
        ("dpi:stage-left", "unknown dpi fn"),
        ("keyboard:layer-up", "unknown keyboard fn"),
        ("macro:1", "unknown family"),
        ("None", "families are lower case"),
    ];
    for (text, why) in bad {
        assert!(parse::binding(text).is_err(), "{text:?}: {why}");
    }
}

#[test]
fn format_binding_writes_modifiers_in_grammar_order() {
    let parsed = parse::binding("key:meta-right+shift+ctrl+KeyA").unwrap();
    assert_eq!(
        parse::format_binding(parsed),
        "key:ctrl+shift+meta-right+KeyA"
    );
}

#[test]
fn binding_round_trips() {
    let forms = [
        "key:KeyA",
        "key:ctrl+KeyA",
        "key:ctrl+shift+alt+meta+meta-right+F12",
        "key:ShiftLeft",
        "mouse:back",
        "mouse:profile-cycle-down",
        "media:player",
        "media:mute",
        "dpi:cycle-up",
        "keyboard:layer-cycle-down",
        "none",
    ];
    for text in forms {
        let parsed = parse::binding(text).unwrap();
        assert_eq!(parse::format_binding(parsed), text, "{text}");
    }
}

#[test]
fn every_function_name_maps_to_its_function_and_back() {
    let cases = [
        ("mouse:left", Binding::Mouse(MouseFn::Left)),
        ("mouse:right", Binding::Mouse(MouseFn::Right)),
        ("mouse:scroll", Binding::Mouse(MouseFn::Scroll)),
        ("mouse:back", Binding::Mouse(MouseFn::Back)),
        ("mouse:forward", Binding::Mouse(MouseFn::Forward)),
        ("mouse:scroll-up", Binding::Mouse(MouseFn::ScrollUp)),
        ("mouse:scroll-down", Binding::Mouse(MouseFn::ScrollDown)),
        (
            "mouse:profile-cycle-up",
            Binding::Mouse(MouseFn::ProfileCycleUp),
        ),
        (
            "mouse:profile-cycle-down",
            Binding::Mouse(MouseFn::ProfileCycleDown),
        ),
        (
            "mouse:battery-status",
            Binding::Mouse(MouseFn::BatteryStatus),
        ),
        ("media:player", Binding::Media(MediaFn::Player)),
        ("media:play-pause", Binding::Media(MediaFn::PlayPause)),
        ("media:next", Binding::Media(MediaFn::Next)),
        ("media:previous", Binding::Media(MediaFn::Previous)),
        ("media:stop", Binding::Media(MediaFn::Stop)),
        ("media:mute", Binding::Media(MediaFn::Mute)),
        ("media:volume-up", Binding::Media(MediaFn::VolumeUp)),
        ("media:volume-down", Binding::Media(MediaFn::VolumeDown)),
        ("dpi:stage-up", Binding::Dpi(DpiFn::StageUp)),
        ("dpi:stage-down", Binding::Dpi(DpiFn::StageDown)),
        ("dpi:cycle-up", Binding::Dpi(DpiFn::CycleUp)),
        ("dpi:cycle-down", Binding::Dpi(DpiFn::CycleDown)),
        (
            "keyboard:profile-cycle-up",
            Binding::Keyboard(KeyboardFn::ProfileCycleUp),
        ),
        (
            "keyboard:profile-cycle-down",
            Binding::Keyboard(KeyboardFn::ProfileCycleDown),
        ),
        (
            "keyboard:layer-cycle-up",
            Binding::Keyboard(KeyboardFn::LayerCycleUp),
        ),
        (
            "keyboard:layer-cycle-down",
            Binding::Keyboard(KeyboardFn::LayerCycleDown),
        ),
    ];
    for (text, binding) in cases {
        assert_eq!(parse::binding(text).unwrap(), binding, "{text}");
        assert_eq!(parse::format_binding(binding), text, "{text}");
    }
}

#[test]
fn numbers_reject_a_leading_plus() {
    assert!(parse::sleep("+5s").is_err(), "+5s");
    assert!(parse::sleep("1m+5s").is_err(), "1m+5s");
    assert!(parse::sleep("+2m").is_err(), "+2m");
    assert!(parse::effect("wave rate=+40").is_err(), "rate=+40");
}

#[test]
fn a_sleep_error_quotes_its_input() {
    let error = parse::sleep("5x").unwrap_err();
    assert!(error.to_string().contains("\"5x\""), "{error}");
    let error = parse::sleep("1mxs").unwrap_err();
    assert!(error.to_string().contains("\"1mxs\""), "{error}");
}

// ---------------------------------------------------------------- effects

#[test]
fn effect_parses_each_name() {
    let cases = [
        ("off", Effect::Off),
        ("glorious rate=10", Effect::Glorious { rate: rate(10) }),
        ("cycle rate=100", Effect::Cycle { rate: rate(100) }),
        (
            "pulse ff0000 0000ff rate=40",
            Effect::Pulse {
                rate: rate(40),
                colors: vec![rgb("ff0000"), rgb("0000ff")],
            },
        ),
        (
            "solid 00ff00",
            Effect::Solid {
                color: rgb("00ff00"),
            },
        ),
        (
            "pulse-one 102030 rate=0",
            Effect::PulseOne {
                rate: rate(0),
                color: rgb("102030"),
            },
        ),
        ("tail rate=55", Effect::Tail { rate: rate(55) }),
        (
            "rave ff0000 rate=40",
            Effect::Rave {
                rate: rate(40),
                colors: vec![rgb("ff0000")],
            },
        ),
        ("wave rate=40", Effect::Wave { rate: rate(40) }),
    ];
    for (text, effect) in cases {
        assert_eq!(parse::effect(text).unwrap(), effect, "{text}");
    }
}

#[test]
fn effect_rate_defaults_to_40() {
    assert_eq!(
        parse::effect("wave").unwrap(),
        Effect::Wave { rate: rate(40) }
    );
    assert_eq!(
        parse::effect("pulse ff0000 0000ff").unwrap(),
        Effect::Pulse {
            rate: rate(40),
            colors: vec![rgb("ff0000"), rgb("0000ff")],
        }
    );
}

#[test]
fn effect_rejects_bad_colour_counts() {
    assert!(parse::effect("pulse ff0000").is_err(), "pulse with 1");
    assert!(
        parse::effect("pulse ff0000 ff0000 ff0000 ff0000 ff0000 ff0000 ff0000").is_err(),
        "pulse with 7"
    );
    assert!(parse::effect("rave").is_err(), "rave with 0");
    assert!(
        parse::effect("rave ff0000 00ff00 0000ff").is_err(),
        "rave with 3"
    );
    assert!(parse::effect("solid").is_err(), "solid with 0");
    assert!(
        parse::effect("solid ff0000 00ff00").is_err(),
        "solid with 2"
    );
    assert!(
        parse::effect("wave ff0000").is_err(),
        "wave takes no colour"
    );
    assert!(parse::effect("off ff0000").is_err(), "off takes no colour");
}

#[test]
fn effect_rejects_a_rate_where_none_exists() {
    assert!(parse::effect("off rate=40").is_err(), "off");
    assert!(parse::effect("solid 00ff00 rate=40").is_err(), "solid");
}

#[test]
fn effect_rejects_malformed_text() {
    let bad = [
        ("", "empty"),
        ("rainbow", "unknown effect"),
        ("wave rate=101", "rate above 100"),
        ("wave rate=", "empty rate"),
        ("wave rate=-1", "negative rate"),
        ("wave rate=40 rate=40", "rate twice"),
        ("wave rate=40 extra", "trailing word"),
        ("pulse rate=40 ff0000 0000ff", "colours after the rate"),
        ("solid gg0000", "bad colour"),
        ("Wave", "names are lower case"),
    ];
    for (text, why) in bad {
        assert!(parse::effect(text).is_err(), "{text:?}: {why}");
    }
}

#[test]
fn format_effect_follows_the_text_form() {
    assert_eq!(parse::format_effect(&Effect::Off), "off");
    assert_eq!(
        parse::format_effect(&Effect::Wave { rate: rate(40) }),
        "wave rate=40"
    );
    assert_eq!(
        parse::format_effect(&Effect::Solid {
            color: rgb("00ff00")
        }),
        "solid 00ff00"
    );
    assert_eq!(
        parse::format_effect(&Effect::Pulse {
            rate: rate(40),
            colors: vec![rgb("FF0000"), rgb("0000ff")],
        }),
        "pulse ff0000 0000ff rate=40"
    );
}

#[test]
fn effect_round_trips() {
    let forms = [
        "off",
        "glorious rate=10",
        "cycle rate=100",
        "pulse ff0000 0000ff 00ff00 rate=40",
        "solid 00ff00",
        "pulse-one 102030 rate=0",
        "tail rate=55",
        "rave ff0000 00ff00 rate=40",
        "wave rate=40",
    ];
    for text in forms {
        let parsed = parse::effect(text).unwrap();
        assert_eq!(parse::format_effect(&parsed), text, "{text}");
    }
}
