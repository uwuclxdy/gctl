#![allow(clippy::unwrap_used, clippy::expect_used)]

//! The motion ladder and the desktop's motion preference: the portal's (or
//! GNOME's) reduce flag and KDE's `AnimationDurationFactor` from kdeglobals.
//! Reduced motion keeps every fade and drops every slide, rise and sink.

use std::time::Duration;

use super::*;

fn ms(value: u64) -> Duration {
    Duration::from_millis(value)
}

fn close(actual: Duration, want: Duration) -> bool {
    (actual.as_secs_f64() - want.as_secs_f64()).abs() < 1e-6
}

// --- the ladder ------------------------------------------------------------------

#[test]
fn the_ladder_matches_the_design_tokens() {
    assert_eq!(TINT, ms(120));
    assert_eq!(NEAR, ms(180));
    assert_eq!(FAR, ms(260));
    assert_eq!(SLIDE, ms(440));
}

#[test]
fn an_exit_runs_at_six_tenths_of_its_entrance() {
    for (entrance, want) in [(TINT, ms(72)), (NEAR, ms(108)), (FAR, ms(156))] {
        assert!(
            close(exit(entrance), want),
            "{entrance:?} -> {:?}",
            exit(entrance)
        );
    }
}

// --- KDE's factor, read from kdeglobals text ----------------------------------------

#[test]
fn the_kde_group_factor_is_read() {
    let text = "[General]\nfont=Onest,10\n\n[KDE]\nLookAndFeelPackage=org.kde.breezedark.desktop\nAnimationDurationFactor=0.75\n\n[WM]\nactiveBlend=0\n";
    assert_eq!(kde_factor(text), Some(0.75));
}

#[test]
fn spaces_around_the_equals_sign_are_trimmed() {
    assert_eq!(
        kde_factor("[KDE]\nAnimationDurationFactor = 0.5\n"),
        Some(0.5)
    );
}

#[test]
fn a_zero_factor_is_read_as_zero() {
    assert_eq!(kde_factor("[KDE]\nAnimationDurationFactor=0\n"), Some(0.0));
}

#[test]
fn the_key_in_another_group_is_not_the_kde_factor() {
    assert_eq!(kde_factor("[General]\nAnimationDurationFactor=0.5\n"), None);
    assert_eq!(
        kde_factor("[KDE]\nSingleClick=false\n[Other]\nAnimationDurationFactor=0.5\n"),
        None
    );
}

#[test]
fn a_missing_or_unusable_factor_is_none() {
    for text in [
        "",
        "[KDE]\n",
        "[KDE]\nAnimationDurationFactor=\n",
        "[KDE]\nAnimationDurationFactor=fast\n",
        "[KDE]\nAnimationDurationFactor=-1\n",
        "[KDE]\nAnimationDurationFactor=NaN\n",
        "[KDE]\nAnimationDurationFactor=inf\n",
        "[KDE]\n# AnimationDurationFactor=0.5\n",
    ] {
        assert_eq!(kde_factor(text), None, "{text:?}");
    }
}

// --- the preference the window runs on ------------------------------------------------

#[test]
fn no_preference_runs_the_design_durations() {
    let prefs = Prefs::from_desktop(false, None);
    assert_eq!(prefs, Prefs::default());
    assert!(!prefs.reduce);
    assert!(close(prefs.fade(NEAR), ms(180)));
    assert!(close(prefs.travel(SLIDE).unwrap(), ms(440)));
}

#[test]
fn the_kde_factor_scales_fades_and_travel() {
    let prefs = Prefs::from_desktop(false, Some(0.75));
    assert!(!prefs.reduce);
    assert!(close(prefs.fade(NEAR), ms(135)));
    assert!(close(prefs.travel(SLIDE).unwrap(), ms(330)));
    assert!(close(prefs.fade(exit(NEAR)), ms(81)));
}

#[test]
fn a_slower_kde_factor_lengthens_motion() {
    let prefs = Prefs::from_desktop(false, Some(2.0));
    assert!(close(prefs.fade(TINT), ms(240)));
    assert!(close(prefs.travel(FAR).unwrap(), ms(520)));
}

#[test]
fn a_zero_kde_factor_reduces_motion_but_keeps_design_length_fades() {
    let prefs = Prefs::from_desktop(false, Some(0.0));
    assert!(prefs.reduce);
    assert_eq!(prefs.travel(SLIDE), None);
    assert!(close(prefs.fade(NEAR), ms(180)));
}

#[test]
fn the_desktop_reduce_flag_drops_travel_and_keeps_scaled_fades() {
    let prefs = Prefs::from_desktop(true, Some(0.75));
    assert!(prefs.reduce);
    assert_eq!(prefs.travel(FAR), None);
    assert!(close(prefs.fade(NEAR), ms(135)));
    let plain = Prefs::from_desktop(true, None);
    assert!(plain.reduce);
    assert_eq!(plain.travel(FAR), None);
    assert!(close(plain.fade(TINT), ms(120)));
}
