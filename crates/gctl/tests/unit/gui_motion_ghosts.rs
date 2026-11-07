#![allow(clippy::unwrap_used, clippy::expect_used)]

//! What leaves the state keeps drawing through its exit: a departing notice
//! and the moved lines hold their last text for the length of the fade.

use std::path::PathBuf;

use iced::time::Instant;

use gctl_proto::PollingRate;

use super::*;
use crate::gui::state::{Edit, Msg, Section, Snapshot};
use crate::gui::testutil::{INSIDE, SETTLED, frame, same, shell as loaded, simulator, snapshot};

fn contains_text(shell: &Shell, text: &str) -> bool {
    simulator(shell.view()).find(text).is_ok()
}

/// A notice that leaves the state keeps drawing its last text through its exit
/// fade, then goes.
#[test]
fn a_departing_notice_keeps_its_text_through_its_exit() {
    let mut shell = loaded(snapshot());
    let start = Instant::now();
    let _ = shell.update(Message::Frame(start));
    let at_rest = frame(&shell);

    let _ = shell.update(Message::Exported {
        device: None,
        result: Ok(PathBuf::from("/x/p.toml")),
    });
    let _ = shell.update(Message::Frame(start + INSIDE));
    assert!(
        !same(&frame(&shell), &at_rest),
        "the notice did not fade in"
    );
    let _ = shell.update(Message::Frame(start + SETTLED));
    let drawn = "Exported to \"/x/p.toml\"";
    assert!(contains_text(&shell, drawn), "the notice is not drawn");

    // an edit clears the footer's slots
    let _ = shell.update(Message::Machine(Msg::Edit(Edit::Debounce(4))));
    let _ = shell.update(Message::Frame(start + SETTLED + INSIDE));
    assert!(
        contains_text(&shell, drawn),
        "the departing notice dropped its text in its exit frame"
    );
    let _ = shell.update(Message::Frame(start + SETTLED + SETTLED + SETTLED));
    assert!(!contains_text(&shell, drawn), "the ghost outlived its exit");
}

/// The moved lines leave the same kind of ghost.
#[test]
fn departing_moved_lines_keep_their_text_through_their_exit() {
    let mut shell = loaded(snapshot());
    let start = Instant::now();
    let _ = shell.update(Message::Machine(Msg::Edit(Edit::PollingRate(
        PollingRate::Hz500,
    ))));
    let _ = shell.update(Message::Machine(Msg::Tick));
    let _ = shell.update(Message::Loaded {
        device: None,
        result: Ok(Snapshot {
            polling_rate: PollingRate::Hz250,
            ..snapshot()
        }),
    });
    let _ = shell.update(Message::Frame(start + SETTLED));
    let line = "Polling rate changed on the mouse to 250 Hz; your edit is kept";
    assert!(contains_text(&shell, line), "the moved line is not drawn");

    let _ = shell.update(Message::Machine(Msg::Select(Section::Lighting)));
    let _ = shell.update(Message::Frame(start + SETTLED + INSIDE));
    assert!(
        contains_text(&shell, line),
        "the departing moved line dropped its text in its exit frame"
    );
    let _ = shell.update(Message::Frame(start + SETTLED + SETTLED + SETTLED));
    assert!(!contains_text(&shell, line), "the ghost outlived its exit");
}
