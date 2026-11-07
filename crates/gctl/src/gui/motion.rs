//! Motion: the duration ladder, the desktop's motion preference, the shell's
//! clock and the easing a value follows while it moves.

use std::time::Duration;

use iced::time::Instant;

/// A state change of colour, opacity or border: nothing travels.
pub(crate) const TINT: Duration = Duration::from_millis(120);
/// Something travelling under 12 px: a toast, a popover, a tooltip.
pub(crate) const NEAR: Duration = Duration::from_millis(180);
/// A panel's worth of travel.
pub(crate) const FAR: Duration = Duration::from_millis(260);
/// A mark travelling a variable distance: the sidebar dot, the selected segment.
pub(crate) const SLIDE: Duration = Duration::from_millis(440);

/// An exit runs at six tenths of its own entrance.
pub(crate) fn exit(entrance: Duration) -> Duration {
    entrance.mul_f32(0.6)
}

/// One shell-driven tint: a section row's selection or a segment's edited mark.
/// The pointer's own surfaces live in the control widget's state instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum Key {
    Segment(u8),
    SectionRow(usize),
}

/// The easing a value follows while it moves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Curve {
    /// State changes: `cubic-bezier(0.2, 0, 0, 1)`.
    Ease,
    /// Entrances and arrivals: `cubic-bezier(0.16, 1, 0.3, 1)`.
    EaseOut,
    /// A mark travelling a variable distance: `cubic-bezier(0, 0.85, 0.35, 1)`.
    Slide,
    /// An exit that accelerates away: `cubic-bezier(0.55, 0, 1, 0.45)`. An open
    /// panel is drawn only while it is most of the way out (its fade reaches
    /// zero a third of the way into the fold), so an exit on [`Curve::Ease`]
    /// would spend most of its duration invisible; this curve holds the panel
    /// drawn for about 60 percent of the close.
    Exit,
}

impl Curve {
    fn points(self) -> (f32, f32, f32, f32) {
        match self {
            Curve::Ease => (0.2, 0.0, 0.0, 1.0),
            Curve::EaseOut => (0.16, 1.0, 0.3, 1.0),
            Curve::Slide => (0.0, 0.85, 0.35, 1.0),
            Curve::Exit => (0.55, 0.0, 1.0, 0.45),
        }
    }

    /// The curve's output for a normalised time `x` in `0..=1`.
    pub(crate) fn at(self, x: f32) -> f32 {
        if x <= 0.0 {
            return 0.0;
        }
        if x >= 1.0 {
            return 1.0;
        }
        let (x1, y1, x2, y2) = self.points();
        // The bezier's x is solved by bisection: a cubic can be non-monotone in
        // x only outside the CSS-valid control range, which every point here is.
        let mut low = 0.0f32;
        let mut high = 1.0f32;
        for _ in 0..24 {
            let t = (low + high) / 2.0;
            if bezier(t, x1, x2) < x {
                low = t;
            } else {
                high = t;
            }
        }
        bezier((low + high) / 2.0, y1, y2)
    }
}

/// One axis of a cubic bezier with fixed endpoints 0 and 1.
fn bezier(t: f32, p1: f32, p2: f32) -> f32 {
    let u = 1.0 - t;
    3.0 * u * u * t * p1 + 3.0 * u * t * t * p2 + t * t * t
}

/// One animated scalar: settled at `to` until `start`, then moving there.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Ease {
    from: f32,
    to: f32,
    start: Option<Instant>,
    duration: Duration,
    curve: Curve,
}

impl Ease {
    /// A value already at rest.
    pub(crate) fn settled(value: f32) -> Ease {
        Ease {
            from: value,
            to: value,
            start: None,
            duration: Duration::ZERO,
            curve: Curve::EaseOut,
        }
    }

    /// Starts moving to `to` at `at`, from wherever the value stands then.
    pub(crate) fn start(&mut self, to: f32, at: Instant, duration: Duration, curve: Curve) {
        self.from = self.at(at);
        self.to = to;
        self.start = Some(at);
        self.duration = duration;
        self.curve = curve;
    }

    /// Starts moving from an explicit `from`, for a value that jumps to a fresh
    /// origin before it moves (a card fading in from zero).
    pub(crate) fn start_from(
        &mut self,
        from: f32,
        to: f32,
        at: Instant,
        duration: Duration,
        curve: Curve,
    ) {
        self.from = from;
        self.to = to;
        self.start = Some(at);
        self.duration = duration;
        self.curve = curve;
    }

    /// The value at `now`.
    pub(crate) fn at(&self, now: Instant) -> f32 {
        let Some(start) = self.start else {
            return self.to;
        };
        let elapsed = now.saturating_duration_since(start).as_secs_f32();
        let total = self.duration.as_secs_f32();
        if total <= 0.0 || elapsed >= total {
            return self.to;
        }
        let progress = self.curve.at(elapsed / total);
        if progress <= 0.0 {
            return self.from;
        }
        self.from + (self.to - self.from) * progress
    }

    /// The value the ease is heading for.
    pub(crate) fn target(&self) -> f32 {
        self.to
    }

    /// Whether the value still moves at `now`.
    pub(crate) fn animating(&self, now: Instant) -> bool {
        match self.start {
            Some(start) => now.saturating_duration_since(start) < self.duration,
            None => false,
        }
    }
}

/// The desktop's motion preference: every duration scales by KDE's factor, and
/// reduce drops every transform while keeping the fades.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Prefs {
    pub(crate) factor: f32,
    pub(crate) reduce: bool,
}

impl Default for Prefs {
    fn default() -> Prefs {
        Prefs {
            factor: 1.0,
            reduce: false,
        }
    }
}

impl Prefs {
    /// The preference the desktop reports: the portal's (or GNOME's) reduce
    /// flag, and KDE's `AnimationDurationFactor` when kdeglobals answers.
    pub(crate) fn from_desktop(portal_reduce: bool, kde: Option<f32>) -> Prefs {
        let factor = match kde {
            Some(factor) if factor.is_finite() && factor >= 0.0 => factor,
            _ => 1.0,
        };
        Prefs {
            factor,
            reduce: portal_reduce || factor == 0.0,
        }
    }

    /// A fade (or any non-travelling change) at the desktop's length. A factor
    /// of 0 still keeps every fade at its design length.
    pub(crate) fn fade(&self, duration: Duration) -> Duration {
        if self.factor <= 0.0 {
            duration
        } else {
            duration.mul_f32(self.factor)
        }
    }

    /// A transform at the desktop's length; `None` under reduced motion, which
    /// drops every slide, rise and sink.
    pub(crate) fn travel(&self, duration: Duration) -> Option<Duration> {
        if self.reduce {
            None
        } else {
            Some(self.fade(duration))
        }
    }
}

/// KDE's `[KDE] AnimationDurationFactor` from kdeglobals text. `None` when the
/// group or key is absent, or the value is not a finite, non-negative number.
/// KConfig lets the last definition of a key override the earlier ones, so the
/// last one wins.
pub(crate) fn kde_factor(text: &str) -> Option<f32> {
    let mut in_kde = false;
    let mut value = None;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        if let Some(group) = line
            .strip_prefix('[')
            .and_then(|rest| rest.strip_suffix(']'))
        {
            in_kde = group.trim() == "KDE";
            continue;
        }
        if !in_kde {
            continue;
        }
        let Some((key, found)) = line.split_once('=') else {
            continue;
        };
        if key_name(key.trim()) != "AnimationDurationFactor" {
            continue;
        }
        value = Some(found.trim());
    }
    let factor = value?.parse::<f32>().ok()?;
    (factor.is_finite() && factor >= 0.0).then_some(factor)
}

/// A KConfig key without its immutability marker: a kiosk-locked or
/// hand-edited `AnimationDurationFactor[$i]` still names that key.
fn key_name(key: &str) -> &str {
    match key
        .strip_suffix(']')
        .and_then(|rest| rest.rsplit_once("[$"))
    {
        Some((name, _)) => name,
        None => key,
    }
}

/// The `[KDE]` factor at `path`, or `None` when the file or the key is missing.
/// Blocking: this runs on the blocking pool, never inside `update`.
pub(crate) fn kde_factor_file(path: &std::path::Path) -> Option<f32> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|text| kde_factor(&text))
}

/// One scalar a widget moves, timed by the `RedrawRequested(at)` instants it is
/// handed: a change starts at the first redraw that sees it, and the clock
/// never moves backwards. Every pointer surface, row costume, trigger edge and
/// the dropdown's open amount is one of these, so none of them needs the shell.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Fade {
    value: f32,
    wanted: f32,
    clock: Option<Instant>,
    moving: Option<(f32, f32, Instant, Duration, Curve)>,
    /// A move waiting for the next redraw: `Some(from)` starts from that value,
    /// `None` from wherever the value stands then.
    pending: Option<(Option<f32>, f32, Duration, Curve)>,
}

impl Fade {
    pub(crate) fn new(value: f32) -> Fade {
        Fade {
            value,
            wanted: value,
            clock: None,
            moving: None,
            pending: None,
        }
    }

    /// Asks to move to `to` from wherever the value stands at the next redraw.
    pub(crate) fn want(&mut self, to: f32, duration: Duration, curve: Curve) {
        if to != self.wanted {
            self.pending = Some((None, to, duration, curve));
            self.wanted = to;
        }
    }

    /// Puts the value at `to` at once, with no move: a control under a prompt
    /// draws at rest the moment the prompt opens.
    pub(crate) fn settle(&mut self, to: f32) {
        self.value = to;
        self.wanted = to;
        self.moving = None;
        self.pending = None;
    }

    /// Takes one `RedrawRequested(at)` instant.
    pub(crate) fn observe(&mut self, at: Instant) {
        let clock = match self.clock {
            Some(clock) if clock >= at => clock,
            _ => at,
        };
        self.clock = Some(clock);
        if let Some((from, to, duration, curve)) = self.pending.take() {
            let from = from.unwrap_or_else(|| self.eval(clock));
            self.moving = Some((from, to, clock, duration, curve));
        }
        self.value = self.eval(clock);
        if let Some((_, to, start, duration, _)) = self.moving
            && clock.saturating_duration_since(start) >= duration
        {
            self.value = to;
            self.moving = None;
        }
    }

    fn eval(&self, now: Instant) -> f32 {
        match self.moving {
            Some((from, to, start, duration, curve)) => {
                let total = duration.as_secs_f32();
                let elapsed = now.saturating_duration_since(start).as_secs_f32();
                if total <= 0.0 || elapsed >= total {
                    to
                } else {
                    from + (to - from) * curve.at(elapsed / total)
                }
            }
            None => self.value,
        }
    }

    pub(crate) fn value(&self) -> f32 {
        self.value
    }

    /// Whether the value is heading somewhere, or waiting for a redraw to.
    pub(crate) fn animating(&self) -> bool {
        self.pending.is_some() || self.moving.is_some()
    }
}

/// The fold of one dropdown: 0 is open, 1 is folded away. A redraw is the only
/// clock; the value never moves backwards.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Fold {
    value: f32,
    clock: Option<Instant>,
    moving: Option<(f32, f32, Instant, Duration, Curve)>,
    pending: Option<(f32, Duration, Curve)>,
    target_open: bool,
}

impl Fold {
    /// A dropdown at rest, folded shut.
    pub(crate) fn new() -> Fold {
        Fold {
            value: 1.0,
            clock: None,
            moving: None,
            pending: None,
            target_open: false,
        }
    }

    /// Asks to unfold. The move starts at the next redraw, so the frame of the
    /// click still draws the shut body.
    pub(crate) fn open(&mut self, duration: Duration, curve: Curve) {
        self.target_open = true;
        self.moving = None;
        self.pending = Some((0.0, duration, curve));
    }

    /// Asks to fold shut. Like [`Fold::open`], the move starts at the first
    /// redraw that follows the ask, from the value the fold stands at then: a
    /// redraw the fold saw seconds ago must not replay as elapsed time and
    /// leave the exit already over on its first frame.
    pub(crate) fn close(&mut self, duration: Duration, curve: Curve) {
        self.target_open = false;
        self.moving = None;
        self.pending = Some((1.0, duration, curve));
    }

    /// Takes one `RedrawRequested(at)` instant.
    pub(crate) fn observe(&mut self, at: Instant) {
        let clock = match self.clock {
            Some(clock) if clock >= at => clock,
            _ => at,
        };
        self.clock = Some(clock);
        if let Some((to, duration, curve)) = self.pending.take() {
            self.move_to(to, clock, duration, curve);
        }
        self.value = self.eval(clock);
        if let Some((_, to, start, duration, _)) = self.moving
            && clock.saturating_duration_since(start) >= duration
        {
            self.value = to;
            self.moving = None;
        }
    }

    fn move_to(&mut self, to: f32, at: Instant, duration: Duration, curve: Curve) {
        self.moving = Some((self.eval(at), to, at, duration, curve));
    }

    fn eval(&self, now: Instant) -> f32 {
        match self.moving {
            Some((from, to, start, duration, curve)) => {
                let total = duration.as_secs_f32();
                let elapsed = now.saturating_duration_since(start).as_secs_f32();
                if total <= 0.0 || elapsed >= total {
                    to
                } else {
                    from + (to - from) * curve.at(elapsed / total)
                }
            }
            None => self.value,
        }
    }

    /// The folded amount at the last redraw: 0 open, 1 shut.
    pub(crate) fn value(&self) -> f32 {
        self.value
    }

    /// Whether the dropdown is asked to be open.
    pub(crate) fn open_target(&self) -> bool {
        self.target_open
    }

    /// Whether the panel still has a move to run or to start.
    pub(crate) fn animating(&self) -> bool {
        self.pending.is_some() || self.moving.is_some()
    }

    /// Whether the panel must stay in the tree so the redraws reach it.
    pub(crate) fn needed(&self) -> bool {
        self.target_open || self.animating()
    }

    /// Puts the fold shut with no move: a prompt covering the dropdown draws
    /// it shut rather than frozen half way, and it comes back shut.
    pub(crate) fn settle(&mut self) {
        self.target_open = false;
        self.value = 1.0;
        self.moving = None;
        self.pending = None;
    }
}

#[cfg(test)]
#[path = "../../tests/unit/gui_motion_prefs.rs"]
mod tests;

#[cfg(test)]
#[path = "../../tests/unit/gui_motion_clock.rs"]
mod clock_tests;
