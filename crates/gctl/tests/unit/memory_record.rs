#![allow(clippy::unwrap_used, clippy::expect_used)]

//! The memory record function (`writes::record_op` / `clear_op`): the one pure
//! function over the remembered values the CLI write path calls. Pins
//! record-on-success, clear-on-failure, the stage-count colour drop and the
//! scroll binds.

use gctl_proto::{
    Binding, Brightness, Button, Dpi, DpiStages, Effect, MouseFn, Profile, Rate, Rgb,
    ScrollDirection,
};

use super::*;
use crate::memory::Memory;

const MODEL: &str = "Model O Wireless";

fn profile(id: u8) -> Profile {
    Profile::new(id).unwrap()
}

fn rgb(hex: &str) -> Rgb {
    hex.parse().unwrap()
}

fn stages(values: &[u16]) -> DpiStages {
    DpiStages::new(values.iter().map(|v| Dpi::new(*v).unwrap()).collect()).unwrap()
}

#[test]
fn record_op_records_each_write_only_value() {
    let mut memory = Memory::default();

    let effect = Effect::Wave {
        rate: Rate::new(40).unwrap(),
    };
    record_op(
        MODEL,
        Some(profile(2)),
        &WriteOp::Effect(effect.clone()),
        None,
        &mut memory,
    );
    assert_eq!(memory.effect(MODEL, profile(2)), Some(&effect));

    let brightness = Brightness {
        wired: 200,
        wireless: 50,
    };
    record_op(
        MODEL,
        None,
        &WriteOp::Brightness(brightness),
        None,
        &mut memory,
    );
    assert_eq!(memory.brightness(MODEL), Some(brightness));

    let colors = vec![rgb("ff0000"), rgb("00ff00")];
    record_op(
        MODEL,
        Some(profile(2)),
        &WriteOp::DpiColors(colors.clone()),
        None,
        &mut memory,
    );
    assert_eq!(
        memory.dpi_colors(MODEL, profile(2)),
        Some(colors.as_slice())
    );

    record_op(
        MODEL,
        Some(profile(2)),
        &WriteOp::Bind(Button::Back, Binding::None),
        None,
        &mut memory,
    );
    assert_eq!(
        memory.bind(MODEL, profile(2), Button::Back),
        Some(Binding::None)
    );
}

#[test]
fn clear_op_clears_each_write_only_value() {
    let mut memory = Memory::default();
    let effect = Effect::Wave {
        rate: Rate::new(40).unwrap(),
    };
    record_op(
        MODEL,
        Some(profile(2)),
        &WriteOp::Effect(effect),
        None,
        &mut memory,
    );
    record_op(
        MODEL,
        None,
        &WriteOp::Brightness(Brightness {
            wired: 200,
            wireless: 50,
        }),
        None,
        &mut memory,
    );
    record_op(
        MODEL,
        Some(profile(2)),
        &WriteOp::DpiColors(vec![rgb("ff0000"), rgb("00ff00")]),
        None,
        &mut memory,
    );
    record_op(
        MODEL,
        Some(profile(2)),
        &WriteOp::Bind(Button::Back, Binding::None),
        None,
        &mut memory,
    );

    clear_op(
        MODEL,
        Some(profile(2)),
        &WriteOp::Effect(Effect::Wave {
            rate: Rate::new(40).unwrap(),
        }),
        None,
        &mut memory,
    );
    clear_op(
        MODEL,
        None,
        &WriteOp::Brightness(Brightness {
            wired: 200,
            wireless: 50,
        }),
        None,
        &mut memory,
    );
    clear_op(
        MODEL,
        Some(profile(2)),
        &WriteOp::DpiColors(vec![rgb("ff0000"), rgb("00ff00")]),
        None,
        &mut memory,
    );
    clear_op(
        MODEL,
        Some(profile(2)),
        &WriteOp::Bind(Button::Back, Binding::None),
        None,
        &mut memory,
    );

    assert_eq!(memory.effect(MODEL, profile(2)), None);
    assert_eq!(memory.brightness(MODEL), None);
    assert_eq!(memory.dpi_colors(MODEL, profile(2)), None);
    assert_eq!(memory.bind(MODEL, profile(2), Button::Back), None);
}

#[test]
fn a_dpi_write_drops_colours_only_when_the_stage_count_changes() {
    let mut memory = Memory::default();
    let colors = vec![rgb("ff0000")];
    memory.profile(MODEL, profile(2)).dpi_colors = Some(colors.clone());

    // Same count: the remembered colours survive the write.
    record_op(
        MODEL,
        Some(profile(2)),
        &WriteOp::Dpi(stages(&[500])),
        Some(1),
        &mut memory,
    );
    assert_eq!(
        memory.dpi_colors(MODEL, profile(2)),
        Some(colors.as_slice())
    );

    // Changed count: the colours no longer line up and are dropped.
    record_op(
        MODEL,
        Some(profile(2)),
        &WriteOp::Dpi(stages(&[500, 800])),
        Some(1),
        &mut memory,
    );
    assert_eq!(memory.dpi_colors(MODEL, profile(2)), None);
}

#[test]
fn a_scroll_write_records_the_six_binds_and_derives_the_direction() {
    let mut memory = Memory::default();
    record_op(
        MODEL,
        None,
        &WriteOp::Scroll(ScrollDirection::Invert),
        None,
        &mut memory,
    );
    assert_eq!(memory.scroll(MODEL), Some(ScrollDirection::Invert));
    for profile in Profile::all() {
        assert_eq!(
            memory.bind(MODEL, profile, Button::ScrollUp),
            Some(Binding::Mouse(MouseFn::ScrollDown))
        );
        assert_eq!(
            memory.bind(MODEL, profile, Button::ScrollDown),
            Some(Binding::Mouse(MouseFn::ScrollUp))
        );
    }

    clear_op(
        MODEL,
        None,
        &WriteOp::Scroll(ScrollDirection::Invert),
        None,
        &mut memory,
    );
    assert_eq!(memory.scroll(MODEL), None);
}

#[test]
fn scroll_binds_maps_default_and_invert() {
    assert_eq!(
        scroll_binds(ScrollDirection::Default),
        (MouseFn::ScrollUp, MouseFn::ScrollDown)
    );
    assert_eq!(
        scroll_binds(ScrollDirection::Invert),
        (MouseFn::ScrollDown, MouseFn::ScrollUp)
    );
}
