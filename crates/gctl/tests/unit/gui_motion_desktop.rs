#![allow(clippy::unwrap_used, clippy::expect_used)]

//! The desktop's motion preference: KDE's override order and immutability
//! markers in kdeglobals, the path resolver, the shell's off-thread re-read of
//! the factor, and the order two answers land in.

use std::ffi::OsString;
use std::path::PathBuf;

use super::*;
use crate::gui::testutil::{shell, snapshot};

fn var(pairs: &'static [(&'static str, &'static str)]) -> impl Fn(&str) -> Option<OsString> {
    move |name| {
        pairs
            .iter()
            .find(|(key, _)| *key == name)
            .map(|(_, value)| OsString::from(*value))
    }
}

// --- kdeglobals text ------------------------------------------------------------

/// KConfig lets the last definition of a key override the earlier one.
#[test]
fn the_last_kde_factor_definition_wins() {
    assert_eq!(
        motion::kde_factor("[KDE]\nAnimationDurationFactor=0.5\nAnimationDurationFactor=0.75\n"),
        Some(0.75)
    );
    assert_eq!(
        motion::kde_factor("[KDE]\nAnimationDurationFactor=0.75\nAnimationDurationFactor=0.5\n"),
        Some(0.5)
    );
}

/// A kiosk-locked key carries KConfig's `[$i]` marker; it still names the
/// factor, and still overrides the unlocked definition before it.
#[test]
fn an_immutable_key_marker_still_names_the_factor() {
    assert_eq!(
        motion::kde_factor("[KDE]\nAnimationDurationFactor[$i]=0.25\n"),
        Some(0.25)
    );
    assert_eq!(
        motion::kde_factor(
            "[KDE]\nAnimationDurationFactor=0.5\nAnimationDurationFactor[$i]=0.25\n"
        ),
        Some(0.25)
    );
    assert_eq!(motion::kde_factor("[KDE]\nSingleClick[$i]=0.25\n"), None);
}

// --- the path and the read ------------------------------------------------------

/// `$XDG_CONFIG_HOME/kdeglobals`, else `$HOME/.config/kdeglobals`; an empty or
/// relative variable counts as unset.
#[test]
fn the_kdeglobals_path_follows_xdg_then_home() {
    assert_eq!(
        crate::kdeglobals_path(&var(&[("XDG_CONFIG_HOME", "/cfg"), ("HOME", "/home/u")])),
        Some(PathBuf::from("/cfg/kdeglobals"))
    );
    assert_eq!(
        crate::kdeglobals_path(&var(&[("XDG_CONFIG_HOME", ""), ("HOME", "/home/u")])),
        Some(PathBuf::from("/home/u/.config/kdeglobals"))
    );
    assert_eq!(
        crate::kdeglobals_path(&var(&[("XDG_CONFIG_HOME", "rel"), ("HOME", "/home/u")])),
        Some(PathBuf::from("/home/u/.config/kdeglobals"))
    );
    assert_eq!(
        crate::kdeglobals_path(&var(&[("HOME", "/home/u")])),
        Some(PathBuf::from("/home/u/.config/kdeglobals"))
    );
    assert_eq!(crate::kdeglobals_path(&var(&[])), None);
}

/// The factor is read off a file, and a missing file is no factor.
#[test]
fn the_kde_factor_is_read_from_a_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("kdeglobals");
    std::fs::write(&path, "[KDE]\nAnimationDurationFactor=0.4\n").unwrap();
    assert_eq!(motion::kde_factor_file(&path), Some(0.4));
    assert_eq!(motion::kde_factor_file(&dir.path().join("missing")), None);
}

/// Regaining focus starts the off-thread read, and no path means no read.
#[test]
fn a_focus_regain_starts_a_kde_read() {
    let mut shell = Shell::new(State::new().0);
    assert_eq!(
        shell
            .update(Message::Window(iced::window::Event::Focused))
            .units(),
        0,
        "no kdeglobals path, nothing to read"
    );
    shell.motion.kdeglobals = Some(PathBuf::from("/cfg/kdeglobals"));
    assert!(
        shell
            .update(Message::Window(iced::window::Event::Focused))
            .units()
            > 0,
        "the re-read never starts"
    );
}

/// The read's answer scales the durations and keeps the portal's flag; a zero
/// factor reduces motion, and a later portal message keeps the factor.
#[test]
fn the_kde_answer_and_the_reduce_flag_meet_in_one_preference() {
    let mut shell = Shell::new(State::new().0);
    let _ = shell.update(Message::Kde {
        sequence: 1,
        factor: Some(0.5),
    });
    assert_eq!(shell.motion.prefs.factor, 0.5);
    assert!(!shell.motion.prefs.reduce);

    let _ = shell.update(Message::Motion(true));
    assert!(shell.motion.prefs.reduce);
    assert_eq!(shell.motion.prefs.factor, 0.5);
    assert_eq!(shell.motion.prefs.travel(motion::NEAR), None);

    let _ = shell.update(Message::Kde {
        sequence: 2,
        factor: Some(0.0),
    });
    assert!(shell.motion.prefs.reduce, "a zero factor reduces motion");
    let _ = shell.update(Message::Motion(false));
    assert!(
        shell.motion.prefs.reduce,
        "the zero factor still reduces it"
    );
}

// --- the answers' order ---------------------------------------------------------

/// Two focus regains start two reads; an older answer that lands after a newer
/// one is dropped, so a stale factor never overwrites the current one.
#[test]
fn a_stale_kde_answer_never_overwrites_a_newer_one() {
    let mut shell = shell(snapshot());
    let _ = shell.update(Message::Kde {
        sequence: 2,
        factor: Some(0.5),
    });
    assert_eq!(shell.motion.prefs.factor, 0.5);
    let _ = shell.update(Message::Kde {
        sequence: 1,
        factor: Some(0.25),
    });
    assert_eq!(
        shell.motion.prefs.factor, 0.5,
        "a stale answer overwrote the newer factor"
    );
    let _ = shell.update(Message::Kde {
        sequence: 3,
        factor: Some(0.75),
    });
    assert_eq!(
        shell.motion.prefs.factor, 0.75,
        "a newer answer was dropped"
    );
}
