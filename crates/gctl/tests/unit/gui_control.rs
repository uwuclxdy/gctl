#![allow(clippy::unwrap_used, clippy::expect_used)]

//! The control widget: a control's hover and press live in its own tree state,
//! read from the cursor on every event and timed by the redraw instants it is
//! handed. These bind the replacement of the shell's per-key eases: a control
//! that never got an exit, or sits under a prompt's scrim, draws at rest by
//! construction.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use iced::time::Instant;
use iced::widget::{container, text};
use iced::{Element, Point, Size, touch};

use super::*;
use crate::gui::motion::Prefs;
use crate::gui::state::Msg;
use crate::gui::testutil::{INSIDE, SETTLED, redraw, same, simulator_sized};
use crate::gui::theme::{self, Scheme};
use crate::gui::view::Message;

const WINDOW: Size = Size::new(300.0, 100.0);
const AT: Point = Point::new(100.0, 18.0);
const AWAY: Point = Point::new(280.0, 90.0);

fn view(frozen: bool, prefs: Prefs) -> Element<'static, Message> {
    Control::new(
        container(text("control").size(14)).padding(8),
        theme::filled_surface(*Scheme::Dark.tokens()),
        prefs,
    )
    .width(200)
    .height(36)
    .frozen(frozen)
    .on_press(Message::Machine(Msg::Tick))
    .into()
}

fn moved(at: Point) -> iced::Event {
    iced::Event::Mouse(iced::mouse::Event::CursorMoved { position: at })
}

fn finger_pressed(at: Point) -> iced::Event {
    iced::Event::Touch(touch::Event::FingerPressed {
        id: touch::Finger(0),
        position: at,
    })
}

fn finger_lifted(at: Point) -> iced::Event {
    iced::Event::Touch(touch::Event::FingerLifted {
        id: touch::Finger(0),
        position: at,
    })
}

fn finger_lost(at: Point) -> iced::Event {
    iced::Event::Touch(touch::Event::FingerLost {
        id: touch::Finger(0),
        position: at,
    })
}

fn ticks(ui: iced_test::Simulator<'_, Message, iced::Theme, iced::Renderer>) -> usize {
    ui.into_messages()
        .filter(|message| matches!(message, Message::Machine(Msg::Tick)))
        .count()
}

/// A pointer leaving the control fades its surface out: with the fade over, the
/// frame is the one a control that was never hovered draws. The widget reads
/// the cursor itself, so no exit message can go missing.
#[test]
fn leaving_the_pointer_fades_the_hover_out() {
    let theme = Scheme::Dark.theme();
    let base = Instant::now() + Duration::from_secs(3600);
    let mut ui = simulator_sized(view(false, Prefs::default()), WINDOW);
    let rest = ui.snapshot(&theme).unwrap();
    ui.point_at(AT);
    let _ = ui.simulate([moved(AT)]);
    let _ = ui.simulate([redraw(base), redraw(base + SETTLED)]);
    let hovered = ui.snapshot(&theme).unwrap();
    assert!(!same(&rest, &hovered), "the control drew no hover surface");
    ui.point_at(AWAY);
    let _ = ui.simulate([moved(AWAY)]);
    let _ = ui.simulate([redraw(base + 2 * SETTLED), redraw(base + 3 * SETTLED)]);
    let left = ui.snapshot(&theme).unwrap();
    assert!(same(&left, &rest), "the hover surface outlived the pointer");
}

/// The hover starts at the first redraw that sees the pointer and moves on the
/// tint, not before: the frame of that redraw is still the resting frame.
#[test]
fn the_hover_starts_at_the_first_redraw_that_sees_it() {
    let theme = Scheme::Dark.theme();
    let base = Instant::now() + Duration::from_secs(3600);
    let mut ui = simulator_sized(view(false, Prefs::default()), WINDOW);
    let rest = ui.snapshot(&theme).unwrap();
    ui.point_at(AT);
    let _ = ui.simulate([moved(AT)]);
    let _ = ui.simulate([redraw(base)]);
    let first = ui.snapshot(&theme).unwrap();
    assert!(
        same(&first, &rest),
        "the hover snapped on in the frame it was asked for"
    );
    let _ = ui.simulate([redraw(base + INSIDE)]);
    let inside = ui.snapshot(&theme).unwrap();
    let _ = ui.simulate([redraw(base + SETTLED)]);
    let settled = ui.snapshot(&theme).unwrap();
    assert!(!same(&inside, &rest), "the hover never moved");
    assert!(
        !same(&inside, &settled),
        "the hover snapped on in one frame"
    );
    assert!(!same(&settled, &rest));
}

/// A control built for a frame under an open prompt draws at rest whatever the
/// pointer did to it: the scrim covers it, and its own state cannot leak.
#[test]
fn a_control_under_a_prompt_draws_at_rest() {
    let theme = Scheme::Dark.theme();
    let base = Instant::now() + Duration::from_secs(3600);
    let mut frozen = simulator_sized(view(true, Prefs::default()), WINDOW);
    let at_rest = frozen.snapshot(&theme).unwrap();
    frozen.point_at(AT);
    let _ = frozen.simulate([moved(AT)]);
    let _ = frozen.simulate([redraw(base), redraw(base + SETTLED)]);
    let pointed = frozen.snapshot(&theme).unwrap();
    assert!(
        same(&at_rest, &pointed),
        "a covered control drew its hover under the prompt"
    );
}

/// A held press sinks the control and a release brings it back.
#[test]
fn a_press_sinks_and_releases() {
    let theme = Scheme::Dark.theme();
    let base = Instant::now() + Duration::from_secs(3600);
    let mut ui = simulator_sized(view(false, Prefs::default()), WINDOW);
    ui.point_at(AT);
    let _ = ui.simulate([moved(AT)]);
    let _ = ui.simulate([redraw(base), redraw(base + SETTLED)]);
    let hovered = ui.snapshot(&theme).unwrap();
    let _ = ui.simulate([iced::Event::Mouse(iced::mouse::Event::ButtonPressed(
        iced::mouse::Button::Left,
    ))]);
    let _ = ui.simulate([redraw(base + 2 * SETTLED), redraw(base + 3 * SETTLED)]);
    let pressed = ui.snapshot(&theme).unwrap();
    assert!(!same(&hovered, &pressed), "the control did not sink");
    let _ = ui.simulate([iced::Event::Mouse(iced::mouse::Event::ButtonReleased(
        iced::mouse::Button::Left,
    ))]);
    let _ = ui.simulate([redraw(base + 4 * SETTLED), redraw(base + 5 * SETTLED)]);
    let released = ui.snapshot(&theme).unwrap();
    assert!(same(&hovered, &released), "the control stayed sunk");
}

/// Reduced motion drops the sink: a held press draws no transform, while the
/// hover fade keeps its length.
#[test]
fn reduced_motion_drops_the_sink() {
    let reduced = Prefs {
        factor: 1.0,
        reduce: true,
    };
    assert_eq!(shift(reduced, 1.0), 0.0);
    assert_eq!(shift(reduced, 0.5), 0.0);
    assert_eq!(shift(Prefs::default(), 1.0), SINK);
    assert_eq!(shift(Prefs::default(), 0.5), SINK / 2.0);
    assert_eq!(reduced.fade(motion::TINT), motion::TINT);
    assert!(Prefs::default().travel(motion::TINT).is_some());
}

/// A content that records the node every `update` and `draw` hands it.
struct Probe {
    seen: Rc<RefCell<Vec<(&'static str, Rectangle)>>>,
}

impl<Message: Clone> Widget<Message, Theme, iced::Renderer> for Probe {
    fn tag(&self) -> tree::Tag {
        tree::Tag::stateless()
    }

    fn state(&self) -> tree::State {
        tree::State::None
    }

    fn size(&self) -> Size<Length> {
        Size {
            width: Length::Fill,
            height: Length::Fill,
        }
    }

    fn layout(
        &mut self,
        _tree: &mut Tree,
        _renderer: &iced::Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        layout::Node::new(limits.resolve(Length::Fill, Length::Fill, Size::ZERO))
    }

    fn draw(
        &self,
        _tree: &Tree,
        _renderer: &mut iced::Renderer,
        _theme: &Theme,
        _style: &renderer::Style,
        layout: Layout<'_>,
        _cursor: mouse::Cursor,
        _viewport: &Rectangle,
    ) {
        self.seen.borrow_mut().push(("draw", layout.bounds()));
    }

    fn update(
        &mut self,
        _tree: &mut Tree,
        _event: &Event,
        layout: Layout<'_>,
        _cursor: mouse::Cursor,
        _renderer: &iced::Renderer,
        _clipboard: &mut dyn Clipboard,
        _shell: &mut Shell<'_, Message>,
        _viewport: &Rectangle,
    ) {
        self.seen.borrow_mut().push(("update", layout.bounds()));
    }
}

/// `update` hands a control's content the node `draw` gives it: its own padded
/// child, never the control's whole node.
#[test]
fn update_hands_the_content_the_same_node_as_draw() {
    let seen = Rc::new(RefCell::new(Vec::new()));
    let control = Control::new(
        Element::new(Probe { seen: seen.clone() }),
        theme::filled_surface(*Scheme::Dark.tokens()),
        Prefs::default(),
    )
    .width(200)
    .height(36)
    .padding(8);
    let mut ui = simulator_sized(control.into(), WINDOW);
    ui.point_at(AT);
    let _ = ui.simulate([moved(AT)]);
    let _ = ui.simulate([redraw(Instant::now() + Duration::from_secs(3600))]);
    let _ = ui.snapshot(&Scheme::Dark.theme()).unwrap();
    let seen = seen.borrow();
    let drawn: Vec<Rectangle> = seen
        .iter()
        .filter(|(kind, _)| *kind == "draw")
        .map(|(_, bounds)| *bounds)
        .collect();
    let updated: Vec<Rectangle> = seen
        .iter()
        .filter(|(kind, _)| *kind == "update")
        .map(|(_, bounds)| *bounds)
        .collect();
    assert!(!drawn.is_empty(), "the content was never drawn");
    assert!(!updated.is_empty(), "the content got no update");
    assert!(
        updated.iter().all(|at| drawn.contains(at)),
        "update handed the content {updated:?}, draw {drawn:?}"
    );
}

/// A tap presses and releases the control like a click: the sink follows the
/// finger, and a lift inside publishes.
#[test]
fn a_tap_presses_and_releases_like_a_click() {
    let theme = Scheme::Dark.theme();
    let base = Instant::now() + Duration::from_secs(3600);
    let mut ui = simulator_sized(view(false, Prefs::default()), WINDOW);
    ui.point_at(AT);
    let _ = ui.simulate([moved(AT)]);
    let _ = ui.simulate([redraw(base), redraw(base + SETTLED)]);
    let hovered = ui.snapshot(&theme).unwrap();
    let _ = ui.simulate([finger_pressed(AT)]);
    let _ = ui.simulate([redraw(base + 2 * SETTLED), redraw(base + 3 * SETTLED)]);
    let pressed = ui.snapshot(&theme).unwrap();
    assert!(
        !same(&hovered, &pressed),
        "the tap did not sink the control"
    );
    let _ = ui.simulate([finger_lifted(AT)]);
    let _ = ui.simulate([redraw(base + 4 * SETTLED), redraw(base + 5 * SETTLED)]);
    let released = ui.snapshot(&theme).unwrap();
    assert!(same(&hovered, &released), "the control stayed sunk");
    assert_eq!(ticks(ui), 1, "a tap published no press");
}

/// A lost finger is a cancel: the sink lets go and the lift publishes nothing.
#[test]
fn a_lost_finger_cancels_the_press() {
    let theme = Scheme::Dark.theme();
    let base = Instant::now() + Duration::from_secs(3600);
    let mut ui = simulator_sized(view(false, Prefs::default()), WINDOW);
    ui.point_at(AT);
    let _ = ui.simulate([moved(AT)]);
    let _ = ui.simulate([redraw(base), redraw(base + SETTLED)]);
    let hovered = ui.snapshot(&theme).unwrap();
    let _ = ui.simulate([finger_pressed(AT)]);
    let _ = ui.simulate([finger_lost(AT)]);
    let _ = ui.simulate([finger_lifted(AT)]);
    let _ = ui.simulate([redraw(base + 2 * SETTLED), redraw(base + 3 * SETTLED)]);
    let released = ui.snapshot(&theme).unwrap();
    assert!(
        same(&hovered, &released),
        "the lost finger left the sink down"
    );
    assert_eq!(ticks(ui), 0, "a lost finger still published the press");
}

/// The shared fade: it starts at the first redraw that follows the ask, never
/// replays an older one, and settles exactly on its target.
#[test]
fn a_fade_starts_at_the_next_redraw_and_never_winds_back() {
    let start = Instant::now();
    let mut fade = Fade::new(0.0);
    fade.want(1.0, Duration::from_millis(120), Curve::Ease);
    assert!(fade.animating());
    assert_eq!(fade.value(), 0.0, "a change starts on the next redraw");
    fade.observe(start);
    assert_eq!(fade.value(), 0.0);
    fade.observe(start + Duration::from_millis(60));
    let middle = fade.value();
    assert!(middle > 0.0 && middle < 1.0, "{middle}");
    fade.observe(start + Duration::from_millis(10));
    assert_eq!(fade.value(), middle, "an older redraw wound the clock back");
    fade.observe(start + Duration::from_millis(120));
    assert_eq!(fade.value(), 1.0);
    assert!(!fade.animating());
    fade.settle(0.0);
    assert_eq!(fade.value(), 0.0, "settle puts the value at rest");
    assert!(!fade.animating());
}
