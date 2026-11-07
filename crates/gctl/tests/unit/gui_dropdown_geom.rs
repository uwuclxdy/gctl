#![allow(clippy::unwrap_used, clippy::expect_used)]

//! The open panel's place, from its pure geometry: the side it opens to and
//! the rect it takes.

use super::*;

const WINDOW: Size = Size::new(1024.0, 680.0);

fn trigger(y: f32) -> Rectangle {
    Rectangle {
        x: 100.0,
        y,
        width: 150.0,
        height: ROW_HEIGHT,
    }
}

/// The panel opens upward only when there is no room below: the polling
/// picker's trigger (y 393 in the shipped window) has 251 px below it, and its
/// 4-row panel needs 144.
#[test]
fn a_panel_with_room_below_opens_below() {
    let t = trigger(393.0);
    assert!(opens_below_for(t, 4, WINDOW));
    let panel = panel_rect(t, 4, true, WINDOW);
    assert!(
        (panel.y - (t.y + t.height)).abs() < 0.5,
        "panel at y {}",
        panel.y
    );
}

/// Wherever its trigger sits in the shipped window, a 4-row panel opens whole
/// (its rows plus the gutter, 152 px) and 6 px clear of the trigger: the side
/// it picks holds the panel and its gap.
#[test]
fn a_panel_opens_whole_and_clear_of_its_trigger() {
    for y in 0..=(WINDOW.height - ROW_HEIGHT) as u32 {
        let t = trigger(y as f32);
        let below = opens_below_for(t, 4, WINDOW);
        let panel = panel_at(t, 4, below, WINDOW);
        assert!(
            (panel.height - 152.0).abs() < 0.5,
            "trigger y {y}: the panel (below {below}) is {} px tall",
            panel.height
        );
        let gap = if below {
            panel.y - (t.y + t.height)
        } else {
            t.y - (panel.y + panel.height)
        };
        assert!(
            (gap - 6.0).abs() < 0.5,
            "trigger y {y}: the panel (below {below}) sits {gap} px from its trigger"
        );
    }
}

/// With no room below for the panel, it opens above.
#[test]
fn a_panel_without_room_below_opens_above() {
    let t = trigger(600.0);
    assert!(!opens_below_for(t, 4, WINDOW));
}

/// However short the window, the panel never covers its own trigger: the seam
/// it slides out of stays where the trigger is.
#[test]
fn the_panel_never_covers_its_trigger() {
    for (height, y) in [
        (400.0, 150.0),
        (420.0, 40.0),
        (420.0, 340.0),
        (680.0, 300.0),
    ] {
        let window = Size::new(1024.0, height);
        let t = trigger(y);
        let below = opens_below_for(t, 90, window);
        let panel = panel_rect(t, 90, below, window);
        let overlap = panel.intersection(&t).map_or(0.0, |r| r.height);
        assert!(
            overlap <= 0.0,
            "window {height}, trigger y {y}: the panel (y {}..{}, below {below}) covers {overlap} px",
            panel.y,
            panel.y + panel.height
        );
        assert!(
            panel.y >= 0.0 && panel.y + panel.height <= height + 0.5,
            "window {height}, trigger y {y}: the panel leaves the window"
        );
    }
}
