#![allow(clippy::unwrap_used, clippy::expect_used)]

//! The marks in the window's own motion: the active-profile dot tracks the
//! active segment, and the dropdown's caret turns over at the fold's midpoint.

use iced::Rectangle;
use iced::time::Instant;

use super::*;
use crate::gui::testutil::{
    INSIDE, SETTLED, frame, profile, same, shell as loaded, simulator, snapshot,
};

// --- the active profile's dot ---------------------------------------------------

/// The sidebar's containers with their ids.
fn sidebar_boxes(shell: &Shell) -> Vec<(Option<iced::widget::Id>, Rectangle)> {
    let mut ui = simulator(shell.view());
    let mut boxes = Vec::new();
    let _ = ui.find(|candidate: iced_test::selector::Candidate<'_>| {
        if let iced_test::selector::Candidate::Container { id, bounds, .. } = candidate
            && bounds.x < 230.0
        {
            boxes.push((id.cloned(), bounds));
        }
        None::<()>
    });
    boxes
}

fn by_id(boxes: &[(Option<iced::widget::Id>, Rectangle)], id: &str) -> Vec<Rectangle> {
    let want = iced::widget::Id::from(id.to_owned());
    boxes
        .iter()
        .filter(|(found, _)| found.as_ref() == Some(&want))
        .map(|(_, bounds)| *bounds)
        .collect()
}

fn centre_x(bounds: &Rectangle) -> f32 {
    bounds.x + bounds.width / 2.0
}

/// The dot's centre x and the active segment's centre x.
fn dot_and_active_segment(shell: &Shell) -> (f32, f32) {
    let boxes = sidebar_boxes(shell);
    let dots = by_id(&boxes, "active-profile-dot");
    assert_eq!(dots.len(), 1, "one active-profile dot, found {dots:?}");
    let active = shell
        .machine
        .snapshot()
        .expect("a loaded snapshot")
        .active
        .get();
    let edited = shell
        .machine
        .edit_profile()
        .expect("an edited profile")
        .get();
    let id = if active == edited {
        "edited-profile".to_owned()
    } else {
        format!("profile-pick-{active}")
    };
    let segments = by_id(&boxes, &id);
    assert_eq!(segments.len(), 1, "one segment {id:?}, found {segments:?}");
    (centre_x(&dots[0]), centre_x(&segments[0]))
}

/// The dot under the active profile is centred on the active segment for every
/// edited/active combination.
#[test]
fn the_active_profile_dot_sits_under_the_active_segment() {
    let mut wrong = Vec::new();
    for active in 1..=3u8 {
        for edited in 1..=3u8 {
            let mut snap = snapshot();
            snap.active = profile(active);
            snap.profile = profile(edited);
            let (dot, segment) = dot_and_active_segment(&loaded(snap));
            if (dot - segment).abs() > 0.5 {
                wrong.push(format!(
                    "active {active}, edited {edited}: {dot} vs {segment}"
                ));
            }
        }
    }
    assert!(wrong.is_empty(), "{wrong:#?}");
}

/// When `Make N active` lands, the dot travels to the new segment as a
/// transform: mid-move it draws away from both resting frames, and settled it
/// draws exactly the new segment's frame.
#[test]
fn the_active_profile_dot_slides_when_make_active_lands() {
    let start = Instant::now();
    let mut shell = loaded(snapshot());
    let _ = shell.update(Message::Frame(start));
    let (dot_before, _) = dot_and_active_segment(&shell);

    let mut landed = snapshot();
    landed.active = profile(1);
    let _ = shell.update(Message::Applied {
        device: None,
        error: None,
        snapshot: Ok(landed.clone()),
        remembered: None,
    });
    let (dot_after, segment_after) = dot_and_active_segment(&loaded(landed.clone()));
    assert!(
        (dot_before - dot_after).abs() > 0.5,
        "the fixture never moved the dot: {dot_before} vs {dot_after}"
    );
    let _ = shell.update(Message::Frame(start + INSIDE));
    let inside = frame(&shell);
    let rest_after = frame(&loaded(landed));
    assert!(
        !same(&inside, &rest_after),
        "the dot jumped to the new segment"
    );
    let _ = shell.update(Message::Frame(start + SETTLED));
    assert!(
        same(&frame(&shell), &rest_after),
        "the dot did not settle under the new segment"
    );
    assert!(
        (dot_after - segment_after).abs() <= 0.5,
        "the settled dot {dot_after} is not under segment {segment_after}"
    );
}

// --- the dropdown caret ---------------------------------------------------------

/// The caret turns over at the fold's midpoint, out of the icon font's own
/// down and up chevrons (the renderer's transformation cannot rotate). The
/// fold is 0 open and 1 shut.
#[test]
fn the_dropdown_caret_turns_over_at_the_folds_midpoint() {
    let down = <iced::Renderer as iced::advanced::text::Renderer>::SCROLL_DOWN_ICON;
    let up = <iced::Renderer as iced::advanced::text::Renderer>::SCROLL_UP_ICON;
    assert_eq!(dropdown::caret(1.0), down, "shut, the caret points down");
    assert_eq!(dropdown::caret(0.6), down, "above the midpoint");
    assert_eq!(dropdown::caret(0.0), up, "open, the caret points up");
    assert_eq!(dropdown::caret(0.4), up, "below the midpoint");
}
