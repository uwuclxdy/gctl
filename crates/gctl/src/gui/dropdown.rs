//! gctl's own dropdown: a field whose open list is a separate panel, held
//! [`PANEL_GAP`] clear of its trigger.
//!
//! iced's `pick_list` cannot animate its menu (its overlay exists only while
//! its state says open, so a close has no frame to run in). This widget keeps
//! the open state and its fade in its own tree state, like `pick_list` keeps
//! `is_open`: its trigger is a caller-built control whose hover and press live
//! in that control's own state, its panel an overlay timed by the redraws it
//! receives. The panel fades in while sliding the gap out of its trigger, and
//! back on close; it never fuses with the trigger, so no seam, shared outline
//! or clamped shadow can break between frames. An open panel's options take
//! clicks at their at-rest place from the frame it opens; a closing panel takes
//! no event at all, so only its exit ghost draws while the events go through to
//! the window under it.

use std::collections::HashMap;
use std::time::Duration;

use iced::advanced::Renderer as _;
use iced::advanced::text::Renderer as _;
use iced::advanced::widget::{Tree, tree};
use iced::advanced::{
    Clipboard, Layout, Overlay, Shell, Widget, layout, mouse, overlay, renderer, text,
};
use iced::{
    Background, Border, Color, Element, Event, Font, Length, Point, Rectangle, Shadow, Size, Theme,
    Vector,
};
use iced::{keyboard, touch, window};

use super::control::{self, SINK};
use super::motion::{self, Curve, Fade, Fold, Prefs};
use super::theme::{self, Scheme};

/// The font size of the panel's labels.
const TEXT_SIZE: f32 = 14.0;
/// One option's row height: the control height, so a panel opens at rest with
/// rows the same size as its trigger.
const ROW_HEIGHT: f32 = theme::CONTROL_HEIGHT;
/// The open panel's cap: a long list scrolls inside it instead of running past
/// the window.
const MAX_PANEL_HEIGHT: f32 = 240.0;
/// The panel's inner gutter around its rows, so the first row's costume never
/// touches the panel's edge.
const PANEL_GUTTER: f32 = 4.0;
/// How far the panel stands clear of its trigger, and so how far it slides out
/// of it while opening.
pub(crate) const PANEL_GAP: f32 = 6.0;
/// The panel's float shadow.
const SHADOW_BLUR: f32 = 18.0;
const SHADOW_OFFSET: f32 = 6.0;
/// The scroll thumb's width, its gap from the panel's edge, and its least
/// height (a 90-row list would otherwise draw a sliver).
const THUMB_WIDTH: f32 = 4.0;
const THUMB_MARGIN: f32 = 4.0;
const THUMB_MIN: f32 = 24.0;

/// The trigger's caret: the icon font's down chevron while the list is closed,
/// its up chevron once the fold passes halfway. The renderer's `Transformation`
/// can only translate and scale — it has no rotation — so the turn is a
/// mirrored glyph swapped at the midpoint, not a rotated one. `fold` is 0 open,
/// 1 shut.
pub(crate) fn caret(fold: f32) -> char {
    if fold >= 0.5 {
        <iced::Renderer as text::Renderer>::SCROLL_DOWN_ICON
    } else {
        <iced::Renderer as text::Renderer>::SCROLL_UP_ICON
    }
}

/// The caret's colour: quiet at rest, one step brighter while the trigger's
/// hover surface is up, on the trigger's own hover fade.
pub(crate) fn caret_colour(t: theme::Tokens, hover: f32) -> Color {
    theme::mix(t.text_4, t.text_3, hover)
}

/// The per-instance state: the fold, its edge fade, the open list's pointer
/// marks and the panel's own geometry. The trigger's hover and press live in
/// the control widget the caller passes as the trigger's face.
#[derive(Debug)]
struct State {
    fold: Fold,
    /// The trigger's accent edge, on the tint ladder. A separate fade, since
    /// the panel itself travels on the near-entrance length.
    edge: Fade,
    /// The key the trigger was pressed with, so only a release inside it opens
    /// the list: `false` once the list is open or the release has landed.
    held: bool,
    highlight: Option<usize>,
    /// The list's scroll offset, in px from the first row.
    scroll: f32,
    /// Whether the panel opens below its field. The panel's layout decides it
    /// once from screen-space bounds; the trigger only reads it.
    opens_below: bool,
    /// Set by the click that opens the menu: the first layout brings the
    /// current value into view.
    fresh_open: bool,
    /// Each row's own costume fade: the row the highlight leaves fades out at
    /// the exit length while the one it takes fades in on the tint.
    rows: HashMap<usize, Fade>,
}

impl State {
    fn new() -> State {
        State {
            fold: Fold::new(),
            edge: Fade::new(0.0),
            held: false,
            highlight: None,
            scroll: 0.0,
            opens_below: false,
            fresh_open: false,
            rows: HashMap::new(),
        }
    }

    /// Follows the highlight: the row it takes fades in, every other row fades
    /// out on its own exit. Returns whether a target moved.
    fn follow_rows(&mut self, options: usize, prefs: Prefs) {
        self.rows.retain(|index, _| *index < options);
        for index in 0..options {
            let fade = self.rows.entry(index).or_insert_with(|| Fade::new(0.0));
            let to = if self.highlight == Some(index) {
                1.0
            } else {
                0.0
            };
            let duration = if to > 0.0 {
                prefs.fade(motion::TINT)
            } else {
                prefs.fade(motion::exit(motion::TINT))
            };
            fade.want(to, duration, Curve::Ease);
        }
    }

    fn row_alpha(&self, index: usize) -> f32 {
        self.rows
            .get(&index)
            .map(|fade| fade.value())
            .unwrap_or(0.0)
    }

    /// Prepares the open that starts now: the highlight returns to the current
    /// value and the list to the top. A costume left from an earlier open is
    /// dropped only when that list had gone before this open; a list reopened
    /// while its close still draws keeps the costume on screen, which
    /// [`State::follow_rows`] then fades out with the highlight.
    fn open(&mut self, selected: Option<usize>, duration: Duration, edge: Duration) {
        let gone = !self.fold.needed();
        self.fold.open(duration, Curve::EaseOut);
        self.edge.want(1.0, edge, Curve::Ease);
        self.highlight = selected;
        self.scroll = 0.0;
        self.fresh_open = true;
        if gone {
            for fade in self.rows.values_mut() {
                fade.settle(0.0);
            }
        }
    }

    /// Puts every fade at rest under an open prompt, and shuts the list: the
    /// panel is never drawn above the modal.
    fn settle(&mut self) {
        self.fold.settle();
        self.edge.settle(0.0);
        self.held = false;
        for fade in self.rows.values_mut() {
            fade.settle(0.0);
        }
    }
}

/// The paint a dropdown shares with the pane it sits in: its scheme, the alpha
/// the pane fades at, and whether an open prompt covers it.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Paint {
    pub(crate) scheme: Scheme,
    pub(crate) alpha: f32,
    pub(crate) frozen: bool,
}

/// A dropdown field: `content` is the caller's trigger face (a control that
/// paints the field and owns its hover), the panel is this widget's overlay.
pub(crate) struct Dropdown<'a, Message> {
    content: Element<'a, Message>,
    options: Vec<String>,
    selected: Option<usize>,
    on_select: Box<dyn FnMut(usize) -> Option<Message> + 'a>,
    font: Font,
    scheme: Scheme,
    alpha: f32,
    frozen: bool,
    prefs: Prefs,
}

impl<'a, Message> Dropdown<'a, Message> {
    pub(crate) fn new(
        paint: Paint,
        prefs: Prefs,
        content: impl Into<Element<'a, Message>>,
        options: Vec<String>,
        selected: Option<usize>,
        font: Font,
        on_select: impl FnMut(usize) -> Option<Message> + 'a,
    ) -> Dropdown<'a, Message> {
        Dropdown {
            content: content.into(),
            options,
            selected,
            on_select: Box::new(on_select),
            font,
            scheme: paint.scheme,
            alpha: paint.alpha,
            frozen: paint.frozen,
            prefs,
        }
    }

    fn duration_open(&self) -> Duration {
        self.prefs.fade(motion::NEAR)
    }

    fn duration_in(&self) -> Duration {
        self.prefs.fade(motion::TINT)
    }
}

impl<Message> Widget<Message, Theme, iced::Renderer> for Dropdown<'_, Message> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State::new())
    }

    fn size(&self) -> Size<Length> {
        self.content.as_widget().size()
    }

    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.content)]
    }

    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(std::slice::from_ref(&self.content));
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
        self.content
            .as_widget_mut()
            .layout(content, renderer, limits)
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut iced::Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        let state = tree.state.downcast_ref::<State>();
        let t = self.scheme.tokens().scale_alpha(self.alpha);
        let Some(content) = tree.children.first() else {
            return;
        };
        // The trigger's hover and sink live in its face's control state.
        let (hover, sink) = if self.frozen {
            (0.0, 0.0)
        } else {
            control::amounts(content)
        };
        let shift = if self.frozen || self.prefs.travel(motion::TINT).is_none() {
            0.0
        } else {
            sink * SINK
        };
        let bounds = layout.bounds();
        let field = Rectangle {
            y: bounds.y + shift,
            ..bounds
        };
        let folded = state.fold.value();
        let edge = if self.frozen { 0.0 } else { state.edge.value() };
        // The dropdown lays its trigger out itself, so its own node is the
        // control's node.
        let content_layout = layout;
        if shift > 0.0 {
            renderer.with_translation(Vector::new(0.0, shift), |renderer| {
                self.content.as_widget().draw(
                    content,
                    renderer,
                    theme,
                    style,
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
                style,
                content_layout,
                cursor,
                viewport,
            );
        }
        // The field's edge is the trigger's one open mark: quiet at rest, on
        // the accent while the list is open, eased on the tint ladder.
        renderer.fill_quad(
            renderer::Quad {
                bounds: field,
                border: theme::trigger_edge(t, edge),
                ..renderer::Quad::default()
            },
            Background::Color(Color::TRANSPARENT),
        );
        // The caret rides the field's right padding, so the label's own room is
        // never taken from it. It brightens with the trigger's hover fade, and
        // it clips where the trigger is visible: a field the pane's edge cuts
        // must not paint its glyph over the pane's neighbour. The layer is the
        // visible field; the viewport handed to the text is the whole field, so
        // the renderer masks the glyph against that layer.
        if let Some(clip) = field.intersection(viewport) {
            renderer.with_layer(clip, |renderer| {
                renderer.fill_text(
                    text::Text {
                        content: caret(folded).to_string(),
                        bounds: Size::new(14.0, field.height),
                        size: 10.0.into(),
                        line_height: text::LineHeight::default(),
                        font: <iced::Renderer as text::Renderer>::ICON_FONT,
                        align_x: text::Alignment::Default,
                        align_y: iced::alignment::Vertical::Center,
                        shaping: text::Shaping::Basic,
                        wrapping: text::Wrapping::default(),
                    },
                    Point::new(field.x + field.width - 18.0, field.center_y()),
                    caret_colour(t, hover),
                    field,
                );
            });
        }
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _renderer: &iced::Renderer,
        _clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        _viewport: &Rectangle,
    ) {
        let state = tree.state.downcast_mut::<State>();
        if self.frozen {
            state.settle();
        }
        if let Some(content) = tree.children.first_mut() {
            self.content.as_widget_mut().update(
                content, event, layout, cursor, _renderer, _clipboard, shell, _viewport,
            );
        }
        if let Event::Window(window::Event::RedrawRequested(at)) = event {
            state.fold.observe(*at);
            state.edge.observe(*at);
            if state.fold.animating() || state.edge.animating() {
                shell.request_redraw();
            }
            return;
        }
        // A control under an open prompt takes nothing at all.
        if self.frozen {
            return;
        }
        let open = state.fold.open_target();
        match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))
            | Event::Touch(touch::Event::FingerPressed { .. })
                if !open && cursor.is_over(layout.bounds()) =>
            {
                // The field sinks under the held press; the list opens on the
                // release, so a press that never lands sinks only.
                state.held = true;
                shell.request_redraw();
                shell.capture_event();
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left))
            | Event::Touch(touch::Event::FingerLifted { .. }) => {
                let held = state.held;
                state.held = false;
                if held && !open && cursor.is_over(layout.bounds()) {
                    state.open(self.selected, self.duration_open(), self.duration_in());
                    shell.request_redraw();
                    shell.capture_event();
                }
            }
            Event::Touch(touch::Event::FingerLost { .. }) => {
                state.held = false;
                shell.request_redraw();
            }
            _ => {}
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
        if self.frozen {
            mouse::Interaction::None
        } else if cursor.is_over(layout.bounds()) {
            mouse::Interaction::Pointer
        } else {
            mouse::Interaction::None
        }
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        _renderer: &iced::Renderer,
        _viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, Theme, iced::Renderer>> {
        let state = tree.state.downcast_mut::<State>();
        if self.frozen {
            state.settle();
            return None;
        }
        if !state.fold.needed() {
            return None;
        }
        let bounds = layout.bounds();
        let trigger = Rectangle {
            x: bounds.x + translation.x,
            y: bounds.y + translation.y,
            ..bounds
        };
        Some(overlay::Element::new(Box::new(Panel {
            state,
            options: &self.options,
            on_select: &mut self.on_select,
            selected: self.selected,
            font: self.font,
            scheme: self.scheme,
            alpha: self.alpha,
            prefs: self.prefs,
            trigger: Rectangle {
                width: trigger.width.max(1.0),
                height: trigger.height.max(1.0),
                ..trigger
            },
        })))
    }
}

impl<'a, Message: 'a> From<Dropdown<'a, Message>> for Element<'a, Message> {
    fn from(dropdown: Dropdown<'a, Message>) -> Element<'a, Message> {
        Element::new(dropdown)
    }
}

/// Whether a panel of `options` rows opens below: whenever the room below
/// holds its height and the gap that keeps it clear of the trigger, else the
/// side with more room.
fn opens_below_for(trigger: Rectangle, options: usize, window: Size) -> bool {
    let below = window.height - (trigger.y + trigger.height);
    below >= panel_size(trigger, options, window).height + PANEL_GAP || below > trigger.y
}

/// The open panel's wanted size: its field's width, its rows plus the gutter,
/// capped at [`MAX_PANEL_HEIGHT`] and at the window it must fit in. The cap
/// leaves the gutter inside it, so a capped panel still shows whole rows.
fn panel_size(trigger: Rectangle, options: usize, window: Size) -> Size {
    let rows = (options as f32 * ROW_HEIGHT).min(MAX_PANEL_HEIGHT - 2.0 * PANEL_GUTTER);
    Size::new(
        trigger.width,
        (rows + 2.0 * PANEL_GUTTER).min(window.height),
    )
}

/// Where the panel sits: against its field on the open side, its height capped
/// by the room that side has, so it never crosses the field and never covers
/// its own trigger.
fn panel_rect(trigger: Rectangle, options: usize, below: bool, window: Size) -> Rectangle {
    let size = panel_size(trigger, options, window);
    let room = if below {
        window.height - (trigger.y + trigger.height)
    } else {
        trigger.y
    }
    .max(0.0);
    let height = size.height.min(room);
    let y = if below {
        trigger.y + trigger.height
    } else {
        trigger.y - height
    };
    Rectangle {
        x: trigger.x.clamp(0.0, (window.width - size.width).max(0.0)),
        y,
        width: size.width,
        height,
    }
}

/// The panel's place on screen: [`panel_rect`]'s side, held [`PANEL_GAP`] clear
/// of the trigger and inside the window.
fn panel_at(trigger: Rectangle, options: usize, below: bool, window: Size) -> Rectangle {
    let rect = panel_rect(trigger, options, below, window);
    if below {
        let y = rect.y + PANEL_GAP;
        Rectangle {
            y,
            height: rect.height.min((window.height - y).max(0.0)),
            ..rect
        }
    } else {
        // `panel_rect`'s above branch ends on the trigger's top edge.
        let bottom = (rect.y + rect.height - PANEL_GAP).max(0.0);
        let height = rect.height.min(bottom);
        Rectangle {
            y: bottom - height,
            height,
            ..rect
        }
    }
}

/// The panel's scrollable list: its rect inside the panel's gutter.
fn content_rect(panel: Rectangle) -> Rectangle {
    Rectangle {
        x: panel.x + PANEL_GUTTER,
        y: panel.y + PANEL_GUTTER,
        width: (panel.width - 2.0 * PANEL_GUTTER).max(0.0),
        height: (panel.height - 2.0 * PANEL_GUTTER).max(0.0),
    }
}

/// How far the list inside `panel` can scroll.
fn max_scroll(options: usize, panel: Rectangle) -> f32 {
    (options as f32 * ROW_HEIGHT - content_rect(panel).height).max(0.0)
}

/// The offset that brings row `index` inside `panel`.
fn scroll_into_view(panel: Rectangle, options: usize, index: usize, scroll: f32) -> f32 {
    let view = content_rect(panel).height;
    let top = index as f32 * ROW_HEIGHT;
    let bottom = top + ROW_HEIGHT;
    let scrolled = if top < scroll {
        top
    } else if bottom > scroll + view {
        bottom - view
    } else {
        scroll
    };
    scrolled.clamp(0.0, max_scroll(options, panel))
}

/// The scroll thumb of an overflowing list, in the pane scrollable's rail
/// colours: a proportional bar at the list's offset, drawn only while the list
/// is taller than the panel.
fn thumb_rect(panel: Rectangle, options: usize, scroll: f32) -> Option<Rectangle> {
    let content = content_rect(panel);
    let max = max_scroll(options, panel);
    if max <= 0.0 || content.height <= 0.0 {
        return None;
    }
    let total = options as f32 * ROW_HEIGHT;
    let height = (content.height * content.height / total)
        .max(THUMB_MIN)
        .min(content.height);
    let travel = content.height - height;
    let y = content.y + (scroll / max).clamp(0.0, 1.0) * travel;
    Some(Rectangle {
        x: content.x + content.width - THUMB_WIDTH - THUMB_MARGIN,
        y,
        width: THUMB_WIDTH,
        height,
    })
}

/// The lane the scroll thumb travels in, the list's full height, while the
/// list scrolls: a press there scrolls nothing and names nothing. A list that
/// fits the panel has no lane and keeps its rows' full width.
fn rail_lane(panel: Rectangle, options: usize, scroll: f32) -> Option<Rectangle> {
    let thumb = thumb_rect(panel, options, scroll)?;
    let content = content_rect(panel);
    Some(Rectangle {
        x: thumb.x,
        width: thumb.width,
        y: content.y,
        height: content.height,
    })
}

/// A row's fill: the row inset from the list's edges, ending before the rail
/// so the thumb never draws over a row's own fill.
fn row_fill(row: Rectangle, rail: bool) -> Rectangle {
    let right = if rail {
        row.width - 4.0 - THUMB_WIDTH - THUMB_MARGIN
    } else {
        row.width - 8.0
    };
    Rectangle {
        x: row.x + 4.0,
        width: right.max(0.0),
        y: row.y + 1.0,
        height: row.height - 2.0,
    }
}

/// The row a press at `position` picks: only inside the list, and never on the
/// scroll rail, which scrolls the list rather than naming an option.
fn row_at(panel: Rectangle, options: usize, scroll: f32, position: Point) -> Option<usize> {
    let content = content_rect(panel);
    if !content.contains(position) {
        return None;
    }
    if let Some(lane) = rail_lane(panel, options, scroll)
        && lane.contains(position)
    {
        return None;
    }
    let offset = position.y - content.y + scroll;
    if offset < 0.0 {
        return None;
    }
    let index = (offset / ROW_HEIGHT) as usize;
    (index < options).then_some(index)
}

/// The cursor an open panel claims over `panel`: a pointer over a row it can
/// pick, the idle cursor over its gutter and rail (still claimed, so nothing
/// under the panel believes the pointer is its own), and nothing outside it.
fn panel_cursor(
    panel: Rectangle,
    options: usize,
    scroll: f32,
    open: bool,
    cursor: mouse::Cursor,
) -> mouse::Interaction {
    if !open || !cursor.is_over(panel) {
        return mouse::Interaction::None;
    }
    match cursor
        .position()
        .and_then(|position| row_at(panel, options, scroll, position))
    {
        Some(_) => mouse::Interaction::Pointer,
        None => mouse::Interaction::Idle,
    }
}

/// The panel's float shadow: always downward, whatever side the panel opened
/// to, so the light direction never flips for one surface.
fn panel_shadow(t: theme::Tokens) -> Shadow {
    Shadow {
        color: t.shadow,
        offset: Vector::new(0.0, SHADOW_OFFSET),
        blur_radius: SHADOW_BLUR,
    }
}

/// A row's costume: the selected row keeps its accent-quiet mark and takes no
/// hover, so the two states never paint on one row; every other row fades the
/// pointer's surface over its own alpha.
fn row_paint(t: theme::Tokens, selected: bool, hover: f32) -> (Option<Color>, Color) {
    if selected {
        (Some(t.accent_quiet), t.accent_text)
    } else if hover > 0.0 {
        (Some(t.surface_2.scale_alpha(hover)), t.text)
    } else {
        (None, t.text)
    }
}

/// The scroll thumb's colour: the pane scrollable's rail at rest.
fn thumb_fill(t: theme::Tokens) -> Color {
    t.surface_2
}

/// The open panel over the window.
struct Panel<'a, Message> {
    state: &'a mut State,
    options: &'a [String],
    on_select: &'a mut dyn FnMut(usize) -> Option<Message>,
    selected: Option<usize>,
    font: Font,
    scheme: Scheme,
    alpha: f32,
    prefs: Prefs,
    trigger: Rectangle,
}

impl<Message> Panel<'_, Message> {
    fn index_at(&self, panel: Rectangle, position: Point) -> Option<usize> {
        row_at(panel, self.options.len(), self.state.scroll, position)
    }

    fn pick(&mut self, index: usize, shell: &mut Shell<'_, Message>) {
        if index >= self.options.len() {
            return;
        }
        if let Some(message) = (self.on_select)(index) {
            shell.publish(message);
        }
        shell.capture_event();
        self.close(shell);
    }

    fn close(&mut self, shell: &mut Shell<'_, Message>) {
        self.state
            .fold
            .close(self.prefs.fade(motion::exit(motion::NEAR)), Curve::Exit);
        self.state.edge.want(
            0.0,
            self.prefs.fade(motion::exit(motion::TINT)),
            Curve::Ease,
        );
        self.state.held = false;
        shell.request_redraw();
    }
}

impl<Message> Overlay<Message, Theme, iced::Renderer> for Panel<'_, Message> {
    fn layout(&mut self, _renderer: &iced::Renderer, bounds: Size) -> layout::Node {
        let below = opens_below_for(self.trigger, self.options.len(), bounds);
        self.state.opens_below = below;
        let panel = panel_at(self.trigger, self.options.len(), below, bounds);
        if self.state.fresh_open {
            self.state.fresh_open = false;
            self.state.scroll = 0.0;
            if let Some(index) = self.state.highlight {
                self.state.scroll = scroll_into_view(panel, self.options.len(), index, 0.0);
            }
        }
        self.state.scroll = self
            .state
            .scroll
            .clamp(0.0, max_scroll(self.options.len(), panel));
        layout::Node::new(panel.size()).move_to(panel.position())
    }

    fn draw(
        &self,
        renderer: &mut iced::Renderer,
        _theme: &Theme,
        _style: &renderer::Style,
        layout: Layout<'_>,
        _cursor: mouse::Cursor,
    ) {
        let fold = self.state.fold.value();
        let open = 1.0 - fold;
        if open <= 0.0 {
            return;
        }
        let t = self.scheme.tokens().scale_alpha(open * self.alpha);
        let panel = layout.bounds();
        let below = self.state.opens_below;
        // The panel slides the gap out of its trigger: the near edge starts on
        // the trigger's own edge and moves away, on the near-entrance length.
        // Reduced motion keeps the fade and drops the travel.
        let slide = match self.prefs.travel(motion::NEAR) {
            Some(_) if fold > 0.0 => Vector::new(
                0.0,
                if below {
                    -fold * PANEL_GAP
                } else {
                    fold * PANEL_GAP
                },
            ),
            _ => Vector::ZERO,
        };
        let at = Rectangle {
            y: panel.y + slide.y,
            ..panel
        };
        // The panel is its own surface: one rounded quad, the quiet line, and
        // its own downward shadow. It holds no seam and clips nothing.
        renderer.fill_quad(
            renderer::Quad {
                bounds: at,
                border: Border {
                    radius: theme::RADIUS_CONTROL.into(),
                    width: 1.0,
                    color: t.line,
                },
                shadow: panel_shadow(t),
                ..renderer::Quad::default()
            },
            Background::Color(t.surface_1),
        );
        let content = content_rect(panel);
        let scroll = self.state.scroll;
        let thumb = thumb_rect(panel, self.options.len(), scroll);
        let rail = thumb.is_some();
        // The rows and the thumb travel together inside the list's clip, so no
        // partly shown row's glyph reaches the panel's border, whether the
        // panel moves or rests.
        let clip = Rectangle {
            y: content.y + slide.y,
            ..content
        };
        renderer.with_layer(clip, |renderer| {
            renderer.with_translation(slide, |renderer| {
                for (index, option) in self.options.iter().enumerate() {
                    let row = Rectangle {
                        y: content.y + index as f32 * ROW_HEIGHT - scroll,
                        height: ROW_HEIGHT,
                        ..content
                    };
                    if row.y + row.height <= content.y || row.y >= content.y + content.height {
                        continue;
                    }
                    // The current value's quiet accent is its one mark: the
                    // pointer's costume never paints on that row.
                    let (fill, colour) =
                        row_paint(t, self.selected == Some(index), self.state.row_alpha(index));
                    if let Some(fill) = fill {
                        renderer.fill_quad(
                            renderer::Quad {
                                bounds: row_fill(row, rail),
                                border: Border {
                                    radius: 8.0.into(),
                                    ..Border::default()
                                },
                                ..renderer::Quad::default()
                            },
                            Background::Color(fill),
                        );
                    }
                    renderer.fill_text(
                        text::Text {
                            content: option.clone(),
                            bounds: Size::new(f32::INFINITY, row.height),
                            size: TEXT_SIZE.into(),
                            line_height: text::LineHeight::default(),
                            font: self.font,
                            align_x: text::Alignment::Default,
                            align_y: iced::alignment::Vertical::Center,
                            shaping: text::Shaping::Advanced,
                            wrapping: text::Wrapping::default(),
                        },
                        Point::new(row.x + 8.0, row.center_y()),
                        colour,
                        // The viewport reaches past the list's clip on
                        // purpose: the renderer only masks cached text against
                        // its layer when the viewport it is handed does.
                        at,
                    );
                }
                if let Some(thumb) = thumb {
                    renderer.fill_quad(
                        renderer::Quad {
                            bounds: thumb,
                            border: Border {
                                radius: 2.0.into(),
                                ..Border::default()
                            },
                            ..renderer::Quad::default()
                        },
                        Background::Color(thumb_fill(t)),
                    );
                }
            });
        });
    }

    fn update(
        &mut self,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _renderer: &iced::Renderer,
        _clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
    ) {
        let panel = layout.bounds();
        if let Event::Window(window::Event::RedrawRequested(at)) = event {
            self.state.fold.observe(*at);
            self.state.edge.observe(*at);
            self.state.follow_rows(self.options.len(), self.prefs);
            for fade in self.state.rows.values_mut() {
                fade.observe(*at);
            }
            if self.state.fold.animating()
                || self.state.edge.animating()
                || self.state.rows.values().any(Fade::animating)
            {
                shell.request_redraw();
            }
            return;
        }
        // A closing panel is only a ghost: it takes no click and no key.
        if !self.state.fold.open_target() {
            return;
        }
        match event {
            Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                if let Some(position) = cursor.position()
                    && let Some(index) = self.index_at(panel, position)
                    && self.state.highlight != Some(index)
                {
                    self.state.highlight = Some(index);
                    shell.request_redraw();
                }
            }
            Event::Mouse(mouse::Event::WheelScrolled { delta }) => {
                if cursor.position_in(panel).is_none() {
                    return;
                }
                let step = match delta {
                    mouse::ScrollDelta::Lines { y, .. } => -y * ROW_HEIGHT,
                    mouse::ScrollDelta::Pixels { y, .. } => -*y,
                };
                let max = max_scroll(self.options.len(), panel);
                self.state.scroll = (self.state.scroll + step).clamp(0.0, max);
                // The row under the pointer is the one the highlight names,
                // however the list moved under it.
                if let Some(position) = cursor.position()
                    && let Some(index) = self.index_at(panel, position)
                {
                    self.state.highlight = Some(index);
                }
                shell.capture_event();
                shell.request_redraw();
            }
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))
            | Event::Touch(touch::Event::FingerPressed { .. }) => {
                if let Some(position) = cursor.position() {
                    if panel.contains(position) {
                        // A press inside the open panel belongs to the panel:
                        // it never closes the list and never reaches what the
                        // panel covers. Only a row's own hit area picks.
                        if let Some(index) = self.index_at(panel, position) {
                            self.state.highlight = Some(index);
                            self.pick(index, shell);
                        }
                        shell.capture_event();
                    } else {
                        self.close(shell);
                        if self.trigger.contains(position) {
                            shell.capture_event();
                        }
                    }
                }
            }
            Event::Keyboard(keyboard::Event::KeyPressed { key, .. }) => match key.as_ref() {
                keyboard::Key::Named(keyboard::key::Named::ArrowUp) => {
                    let current = self.state.highlight.unwrap_or(self.options.len());
                    let index = current.saturating_sub(1);
                    self.state.highlight = Some(index);
                    self.state.scroll =
                        scroll_into_view(panel, self.options.len(), index, self.state.scroll);
                    shell.capture_event();
                    shell.request_redraw();
                }
                keyboard::Key::Named(keyboard::key::Named::ArrowDown) => {
                    let last = self.options.len().saturating_sub(1);
                    let index = self
                        .state
                        .highlight
                        .map_or(0, |current| (current + 1).min(last));
                    self.state.highlight = Some(index);
                    self.state.scroll =
                        scroll_into_view(panel, self.options.len(), index, self.state.scroll);
                    shell.capture_event();
                    shell.request_redraw();
                }
                keyboard::Key::Named(keyboard::key::Named::Enter) => {
                    if let Some(index) = self.state.highlight.or(self.selected) {
                        self.pick(index, shell);
                    }
                }
                keyboard::Key::Named(keyboard::key::Named::Escape) => {
                    self.close(shell);
                    shell.capture_event();
                }
                _ => {}
            },
            _ => {}
        }
    }

    fn mouse_interaction(
        &self,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _renderer: &iced::Renderer,
    ) -> mouse::Interaction {
        panel_cursor(
            layout.bounds(),
            self.options.len(),
            self.state.scroll,
            self.state.fold.open_target(),
            cursor,
        )
    }
}

#[cfg(test)]
#[path = "../../tests/unit/gui_dropdown.rs"]
mod geometry_tests;

#[cfg(test)]
#[path = "../../tests/unit/gui_dropdown_geom.rs"]
mod placement_tests;
