#![allow(clippy::unwrap_used, clippy::expect_used)]

//! The shell's clock semantics and the dropdown's fold: an eased scalar settles
//! exactly on its target, and a fold's clock only ever moves forward.

use std::time::Duration;

use iced::time::Instant;

use super::*;

fn ms(value: u64) -> Duration {
    Duration::from_millis(value)
}

#[test]
fn an_eased_value_starts_exactly_and_settles_exactly() {
    let start = Instant::now();
    let mut ease = Ease::settled(4.0);
    assert_eq!(ease.at(start), 4.0);
    assert!(!ease.animating(start));

    ease.start(10.0, start, ms(180), Curve::EaseOut);
    assert_eq!(ease.at(start), 4.0);
    assert!(ease.animating(start));
    assert!(ease.at(start + ms(90)) > 4.0 && ease.at(start + ms(90)) < 10.0);
    assert_eq!(ease.at(start + ms(180)), 10.0);
    assert_eq!(ease.at(start + ms(10_000)), 10.0);
    assert!(!ease.animating(start + ms(180)));
}

#[test]
fn one_value_never_moves_backwards_when_a_new_move_interrupts_it() {
    let start = Instant::now();
    let mut ease = Ease::settled(0.0);
    ease.start(100.0, start, ms(440), Curve::Slide);
    let middle = ease.at(start + ms(100));
    ease.start(0.0, start + ms(100), ms(440), Curve::Ease);
    assert_eq!(ease.at(start + ms(100)), middle);
}

/// The curves are pinned at two interior points, computed by an independent
/// cubic-bezier solver over each curve's control points (not by this code):
/// `Ease` (0.2, 0, 0, 1) 0.25 -> 0.6072, 0.5 -> 0.8778;
/// `EaseOut` (0.16, 1, 0.3, 1) 0.25 -> 0.8256, 0.5 -> 0.9718;
/// `Slide` (0, 0.85, 0.35, 1) 0.25 -> 0.8133, 0.5 -> 0.9454;
/// `Exit` (0.55, 0, 1, 0.45) 0.25 -> 0.0318, 0.5 -> 0.1349.
#[test]
fn the_curves_hold_their_hand_computed_points() {
    let within = |curve: Curve, x: f32, want: f32| {
        let got = curve.at(x);
        assert!(
            (got - want).abs() <= 0.002,
            "{curve:?} at {x}: {got}, want {want}"
        );
    };
    for curve in [Curve::Ease, Curve::EaseOut, Curve::Slide, Curve::Exit] {
        assert_eq!(curve.at(0.0), 0.0, "{curve:?}");
        assert_eq!(curve.at(1.0), 1.0, "{curve:?}");
        assert_eq!(curve.at(-1.0), 0.0, "{curve:?}");
        assert_eq!(curve.at(2.0), 1.0, "{curve:?}");
    }
    within(Curve::Ease, 0.25, 0.6072);
    within(Curve::Ease, 0.5, 0.8778);
    within(Curve::EaseOut, 0.25, 0.8256);
    within(Curve::EaseOut, 0.5, 0.9718);
    within(Curve::Slide, 0.25, 0.8133);
    within(Curve::Slide, 0.5, 0.9454);
    within(Curve::Exit, 0.25, 0.0318);
    within(Curve::Exit, 0.5, 0.1349);
    for curve in [Curve::Ease, Curve::EaseOut, Curve::Slide, Curve::Exit] {
        assert!(curve.at(0.1) < curve.at(0.5), "{curve:?}");
        assert!(curve.at(0.5) < curve.at(0.9), "{curve:?}");
    }
}

#[test]
fn a_fold_shut_grows_a_move_only_after_the_first_redraw() {
    let mut fold = Fold::new();
    assert_eq!(fold.value(), 1.0);
    fold.open(ms(180), Curve::EaseOut);
    assert!(fold.needed());
    assert_eq!(fold.value(), 1.0, "a click frame still draws the shut body");

    let first = Instant::now();
    fold.observe(first);
    assert_eq!(fold.value(), 1.0, "the fold starts at the first redraw");
    assert!(fold.animating());

    fold.observe(first + ms(180));
    assert_eq!(fold.value(), 0.0);
    assert!(!fold.animating());
}

/// The close starts at the first redraw after the ask, whatever redraw the fold
/// last saw: a close asked for seconds after the last frame still folds on the
/// frames that follow it, and its ask frame still draws the open panel.
#[test]
fn a_fold_shut_starts_at_the_first_redraw_after_the_ask() {
    let mut fold = Fold::new();
    fold.open(ms(180), Curve::EaseOut);
    let first = Instant::now();
    fold.observe(first);
    fold.observe(first + ms(180));
    assert_eq!(fold.value(), 0.0);
    assert!(!fold.animating());

    fold.close(ms(108), Curve::Ease);
    assert!(fold.needed(), "a closing panel left the tree at the ask");
    assert_eq!(
        fold.value(),
        0.0,
        "the ask frame still draws the open panel"
    );
    let idle = first + ms(180) + ms(5000);
    fold.observe(idle);
    assert_eq!(
        fold.value(),
        0.0,
        "the close started at a stale redraw and was already over"
    );
    assert!(fold.animating());
    fold.observe(idle + ms(54));
    let middle = fold.value();
    assert!(middle > 0.0 && middle < 1.0, "{middle}");
    fold.observe(idle + ms(108));
    assert_eq!(fold.value(), 1.0);
    assert!(!fold.animating());
    assert!(!fold.needed());
}

#[test]
fn a_fold_ignores_an_older_redraw() {
    let mut fold = Fold::new();
    fold.open(ms(180), Curve::EaseOut);
    let first = Instant::now();
    fold.observe(first);
    fold.observe(first + ms(90));
    let middle = fold.value();
    assert!(middle > 0.0 && middle < 1.0, "{middle}");
    fold.observe(first + ms(20));
    assert_eq!(
        fold.value(),
        middle,
        "an older redraw never winds the clock back"
    );
    fold.observe(first + ms(1000));
    assert_eq!(fold.value(), 0.0);
}
