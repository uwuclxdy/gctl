#![allow(clippy::unwrap_used, clippy::expect_used)]

//! The shell's own tints: a section row and the edited digit ease onto their
//! selection instead of snapping to it.

use std::time::Duration;

use iced::time::Instant;

use super::*;
use crate::gui::state::{Msg, Section, Snapshot};
use crate::gui::testutil::{SETTLED, profile, shell, snapshot};

// --- the section row's tint -----------------------------------------------------

/// The open section row's fill and label ease on the tint ladder, landing with
/// the dot that slid to them instead of snapping 440 ms ahead of it.
#[test]
fn the_section_rows_tint_eases_in() {
    let mut shell = shell(snapshot());
    let base = Instant::now() + Duration::from_secs(3600);
    let _ = shell.update(Message::Frame(base));
    let _ = shell.update(Message::Machine(Msg::Select(Section::Lighting)));
    let now = shell.motion.now();
    assert!(
        shell
            .motion
            .selection_at(Key::SectionRow(1), true, now)
            .abs()
            < f32::EPSILON,
        "the new row's tint jumped on the frame of the change"
    );
    let _ = shell.update(Message::Frame(base + Duration::from_millis(40)));
    let now = shell.motion.now();
    let mid = shell.motion.selection_at(Key::SectionRow(1), true, now);
    assert!(mid > 0.0 && mid < 1.0, "the tint is at {mid} 40 ms in");
    assert!(shell.motion.selection_at(Key::SectionRow(0), false, now) < 1.0);
    let _ = shell.update(Message::Frame(base + SETTLED));
    let now = shell.motion.now();
    assert!((shell.motion.selection_at(Key::SectionRow(1), true, now) - 1.0).abs() < f32::EPSILON);
    assert!(
        shell
            .motion
            .selection_at(Key::SectionRow(0), false, now)
            .abs()
            < f32::EPSILON
    );
}

/// The edited digit's colour eases onto its capsule: it does not snap to the
/// selected tone on the frame of the pick.
#[test]
fn the_edited_digit_tones_in_with_its_capsule() {
    let mut shell = shell(snapshot());
    let base = Instant::now() + Duration::from_secs(3600);
    let _ = shell.update(Message::Frame(base));
    let _ = shell.update(Message::Machine(Msg::PickProfile(profile(3))));
    let _ = shell.update(Message::Loaded {
        device: None,
        result: Ok(Snapshot {
            profile: profile(3),
            ..snapshot()
        }),
    });
    let now = shell.motion.now();
    assert!(shell.motion.selection_at(Key::Segment(3), true, now).abs() < f32::EPSILON);
    let _ = shell.update(Message::Frame(base + Duration::from_millis(40)));
    let now = shell.motion.now();
    let mid = shell.motion.selection_at(Key::Segment(3), true, now);
    assert!(mid > 0.0 && mid < 1.0, "the digit's tone is at {mid}");
    let _ = shell.update(Message::Frame(base + SETTLED));
    let now = shell.motion.now();
    assert!((shell.motion.selection_at(Key::Segment(3), true, now) - 1.0).abs() < f32::EPSILON);
    assert!(shell.motion.selection_at(Key::Segment(2), false, now).abs() < f32::EPSILON);
}
