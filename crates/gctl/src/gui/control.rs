//! A control's pointer state in its own widget tree: the hover surface fades in
//! and out and a held press sinks the control, both timed by the redraw
//! instants the widget is handed. The cursor is read on every event, so a
//! control that turns disabled, leaves the window, or sits under an open
//! prompt's scrim loses its hover without anyone telling it, and a control that
//! vanishes takes its state with the tree node.

use iced::advanced::Renderer as _;
use iced::advanced::widget::{Operation, Tree, tree};
use iced::advanced::{Clipboard, Layout, Shell, Widget, layout, mouse, renderer};
use iced::widget::button;
use iced::{Element, Event, Length, Padding, Rectangle, Size, Theme, Vector, touch, window};

use super::motion::{self, Curve, Fade, Prefs};
use super::theme::Surface;

/// How far a pressed control sinks.
pub(crate) const SINK: f32 = 1.5;

/// How far a control draws from its layout: its eased sink, unless reduced
/// motion drops every transform.
fn shift(prefs: Prefs, sink: f32) -> f32 {
    if prefs.travel(motion::TINT).is_none() {
        0.0
    } else {
        sink * SINK
    }
}

/// The pointer state: two eased scalars on the redraw clock, which one is held,
/// and whether the last event saw the cursor inside.
#[derive(Debug)]
struct State {
    hover: Fade,
    sink: Fade,
    over: bool,
    held: bool,
}

impl State {
    fn new() -> State {
        State {
            hover: Fade::new(0.0),
            sink: Fade::new(0.0),
            over: false,
            held: false,
        }
    }

    /// Follows the cursor at `over`: the surface fades in on the tint and out
    /// on its exit. Returns whether the target moved.
    fn follow(&mut self, over: bool, prefs: Prefs) -> bool {
        if over == self.over {
            return false;
        }
        self.over = over;
        let duration = if over {
            prefs.fade(motion::TINT)
        } else {
            prefs.fade(motion::exit(motion::TINT))
        };
        self.hover
            .want(if over { 1.0 } else { 0.0 }, duration, Curve::Ease);
        true
    }

    /// Holds the sink at `held`, or settles it where reduced motion draws it.
    fn press(&mut self, held: bool, prefs: Prefs) {
        self.held = held;
        if prefs.travel(motion::TINT).is_none() {
            self.sink.settle(0.0);
            return;
        }
        let duration = if held {
            prefs.fade(motion::TINT)
        } else {
            prefs.fade(motion::exit(motion::TINT))
        };
        self.sink
            .want(if held { 1.0 } else { 0.0 }, duration, Curve::Ease);
    }
}

/// One control: `content` is its face, `surface` paints under it from the eased
/// hover and the control's state, and `on_press` is what a release inside it
/// publishes. A control with no message is a surface only (a picker's field).
pub(crate) struct Control<'a, Message> {
    content: Element<'a, Message>,
    surface: Surface<'a>,
    on_press: Option<Message>,
    prefs: Prefs,
    disabled: bool,
    frozen: bool,
    width: Length,
    height: Length,
    padding: Padding,
}

impl<'a, Message> Control<'a, Message> {
    pub(crate) fn new(
        content: impl Into<Element<'a, Message>>,
        surface: Surface<'a>,
        prefs: Prefs,
    ) -> Control<'a, Message> {
        let content = content.into();
        let size = content.as_widget().size_hint();
        Control {
            content,
            surface,
            on_press: None,
            prefs,
            disabled: false,
            frozen: false,
            width: size.width.fluid(),
            height: size.height.fluid(),
            padding: button::DEFAULT_PADDING,
        }
    }

    pub(crate) fn on_press(mut self, message: Message) -> Control<'a, Message> {
        self.on_press = Some(message);
        self
    }

    /// A control drawn in its disabled state: no hover, no sink, no message.
    pub(crate) fn disabled(mut self, disabled: bool) -> Control<'a, Message> {
        self.disabled = disabled;
        self
    }

    /// A control in the base layer of an open prompt: it draws at rest and
    /// takes no pointer, since the scrim covers it.
    pub(crate) fn frozen(mut self, frozen: bool) -> Control<'a, Message> {
        self.frozen = frozen;
        self
    }

    pub(crate) fn width(mut self, width: impl Into<Length>) -> Control<'a, Message> {
        self.width = width.into();
        self
    }

    pub(crate) fn height(mut self, height: impl Into<Length>) -> Control<'a, Message> {
        self.height = height.into();
        self
    }

    pub(crate) fn padding(mut self, padding: impl Into<Padding>) -> Control<'a, Message> {
        self.padding = padding.into();
        self
    }
}

/// A child control's eased hover and sink, for a parent that must draw with it
/// (the dropdown's caret and edge ride its field). `(0, 0)` when `tree` is not
/// a control.
pub(crate) fn amounts(tree: &Tree) -> (f32, f32) {
    if tree.tag != tree::Tag::of::<State>() {
        return (0.0, 0.0);
    }
    let state = tree.state.downcast_ref::<State>();
    (state.hover.value(), state.sink.value())
}

impl<Message: Clone> Widget<Message, Theme, iced::Renderer> for Control<'_, Message> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State::new())
    }

    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.content)]
    }

    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(std::slice::from_ref(&self.content));
    }

    fn size(&self) -> Size<Length> {
        Size {
            width: self.width,
            height: self.height,
        }
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &iced::Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        let Some(content) = tree.children.first_mut() else {
            return layout::Node::new(Size::ZERO);
        };
        layout::padded(limits, self.width, self.height, self.padding, |limits| {
            self.content
                .as_widget_mut()
                .layout(content, renderer, limits)
        })
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &iced::Renderer,
        operation: &mut dyn Operation,
    ) {
        operation.container(None, layout.bounds());
        operation.traverse(&mut |operation| {
            if let Some(content) = tree.children.first_mut() {
                self.content.as_widget_mut().operate(
                    content,
                    layout.children().next().unwrap_or(layout),
                    renderer,
                    operation,
                );
            }
        });
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut iced::Renderer,
        theme: &Theme,
        _style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        let bounds = layout.bounds();
        let state = tree.state.downcast_ref::<State>();
        let idle = self.disabled || self.frozen;
        let (hover, sink, held) = if idle {
            (0.0, 0.0, false)
        } else {
            (state.hover.value(), state.sink.value(), state.held)
        };
        let status = if self.disabled {
            button::Status::Disabled
        } else if held {
            button::Status::Pressed
        } else {
            button::Status::Active
        };
        let surface = (self.surface)(hover, status);
        let offset = if idle { 0.0 } else { shift(self.prefs, sink) };
        let at = Rectangle {
            y: bounds.y + offset,
            ..bounds
        };
        if surface.background.is_some()
            || surface.border.width > 0.0
            || surface.shadow.color.a > 0.0
        {
            renderer.fill_quad(
                renderer::Quad {
                    bounds: at,
                    border: surface.border,
                    shadow: surface.shadow,
                    snap: surface.snap,
                },
                surface
                    .background
                    .unwrap_or(iced::Background::Color(iced::Color::TRANSPARENT)),
            );
        }
        let Some(content) = tree.children.first() else {
            return;
        };
        let child_style = renderer::Style {
            text_color: surface.text_color,
        };
        let content_layout = layout.children().next().unwrap_or(layout);
        if offset != 0.0 {
            renderer.with_translation(Vector::new(0.0, offset), |renderer| {
                self.content.as_widget().draw(
                    content,
                    renderer,
                    theme,
                    &child_style,
                    content_layout,
                    cursor,
                    viewport,
                );
            });
        } else {
            self.content.as_widget().draw(
                content,
                renderer,
                theme,
                &child_style,
                content_layout,
                cursor,
                viewport,
            );
        }
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &iced::Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        let bounds = layout.bounds();
        let state = tree.state.downcast_mut::<State>();
        if self.disabled || self.frozen {
            state.hover.settle(0.0);
            state.sink.settle(0.0);
            state.over = false;
            state.held = false;
        } else {
            let over = cursor.is_over(bounds);
            if state.follow(over, self.prefs) {
                shell.request_redraw();
            }
            match event {
                Event::Window(window::Event::RedrawRequested(at)) => {
                    state.hover.observe(*at);
                    state.sink.observe(*at);
                    if state.hover.animating() || state.sink.animating() {
                        shell.request_redraw();
                    }
                }
                Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))
                | Event::Touch(touch::Event::FingerPressed { .. })
                    if state.over =>
                {
                    state.press(true, self.prefs);
                    shell.request_redraw();
                }
                Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left))
                | Event::Touch(touch::Event::FingerLifted { .. })
                    if state.held =>
                {
                    state.press(false, self.prefs);
                    if state.over
                        && let Some(message) = &self.on_press
                    {
                        shell.publish(message.clone());
                    }
                    shell.request_redraw();
                }
                Event::Touch(touch::Event::FingerLost { .. }) if state.held => {
                    state.press(false, self.prefs);
                    shell.request_redraw();
                }
                _ => {}
            }
        }
        if let Some(content) = tree.children.first_mut() {
            self.content.as_widget_mut().update(
                content,
                event,
                layout.children().next().unwrap_or(layout),
                cursor,
                renderer,
                clipboard,
                shell,
                viewport,
            );
        }
    }

    fn mouse_interaction(
        &self,
        _tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _viewport: &Rectangle,
        _renderer: &iced::Renderer,
    ) -> mouse::Interaction {
        if !self.disabled && !self.frozen && cursor.is_over(layout.bounds()) {
            mouse::Interaction::Pointer
        } else {
            mouse::Interaction::None
        }
    }
}

impl<'a, Message: Clone + 'a> From<Control<'a, Message>> for Element<'a, Message> {
    fn from(control: Control<'a, Message>) -> Element<'a, Message> {
        Element::new(control)
    }
}

#[cfg(test)]
#[path = "../../tests/unit/gui_control.rs"]
mod tests;
