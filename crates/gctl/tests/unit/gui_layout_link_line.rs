#![allow(clippy::unwrap_used, clippy::expect_used)]

//! The link line holds one height: the battery's tag never moves the group
//! below it.

use gctl_proto::{Battery, DeviceState};

use super::*;
use crate::gui::testutil::{shell, simulator, snapshot};

fn texts(shell: &Shell) -> Vec<(String, iced::Rectangle)> {
    let mut ui = simulator(shell.view());
    let mut found = Vec::new();
    let _ = ui.find(|candidate: iced_test::selector::Candidate<'_>| {
        if let iced_test::selector::Candidate::Text {
            content, bounds, ..
        } = candidate
        {
            found.push((content.to_owned(), bounds));
        }
        None::<()>
    });
    found
}

/// The `Unknown` battery tag is taller than the meter, so the link line holds
/// one height: the profile group below it sits at the same y either way.
#[test]
fn the_link_line_holds_one_height_with_or_without_a_reading() {
    let known = shell(snapshot());
    let unknown = shell(Snapshot {
        battery: Battery {
            state: DeviceState::Awake,
            percent: None,
        },
        ..snapshot()
    });
    let eyebrow = eyebrow("Profile");
    let y = |shell: &Shell| {
        texts(shell)
            .into_iter()
            .find(|(text, _)| *text == eyebrow)
            .map(|(_, bounds)| bounds.y)
            .expect("no PROFILE eyebrow")
    };
    let (a, b) = (y(&known), y(&unknown));
    assert!(
        (a - b).abs() < 0.5,
        "the profile group sits at {a} with a reading and {b} without"
    );
}
