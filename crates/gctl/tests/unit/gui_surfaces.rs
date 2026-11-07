#![allow(clippy::unwrap_used, clippy::expect_used)]

//! Every surface a control paints carries the radius of the shape that control
//! draws: the radius ladder, checked at each constructor rather than through a
//! pixel frame, so an unrounded surface cannot ship green again.

use super::*;
use iced::widget::button::Status;

/// The four corners of a border radius, in declaration order.
fn corners(radius: iced::border::Radius) -> [f32; 4] {
    [
        radius.top_left,
        radius.top_right,
        radius.bottom_right,
        radius.bottom_left,
    ]
}

/// Every `Surface` a control can paint, at rest and under the pointer, in every
/// status: each carries the radius of the shape it fills.
#[test]
fn every_control_surface_carries_the_ladder_radius() {
    let mut square = Vec::new();
    for scheme in [Scheme::Dark, Scheme::Light] {
        let t = *scheme.tokens();
        let surfaces: [(&str, Surface<'static>, f32); 9] = [
            ("filled", filled_surface(t), RADIUS_CONTROL),
            ("primary", primary_surface(t), RADIUS_CONTROL),
            ("secondary", secondary_surface(t), RADIUS_CONTROL),
            ("warning", warning_surface(t), RADIUS_CONTROL),
            ("danger", danger_surface(t), RADIUS_CONTROL),
            ("quiet", quiet_surface(t), RADIUS_CONTROL),
            ("nav", nav_surface(t, 0.0), 8.0),
            ("segment", segment_surface(t, true), 8.0),
            ("field", field_surface(t), RADIUS_CONTROL),
        ];
        for (name, surface, want) in surfaces {
            for hover in [0.0, 1.0] {
                for status in [
                    Status::Active,
                    Status::Hovered,
                    Status::Pressed,
                    Status::Disabled,
                ] {
                    let got = corners(surface(hover, status).border.radius);
                    if got != [want; 4] {
                        square.push(format!(
                            "{scheme:?} {name} at hover {hover} {status:?}: {got:?}, want {want}"
                        ));
                    }
                }
            }
        }
    }
    assert!(
        square.is_empty(),
        "surfaces without the ladder radius: {square:#?}"
    );
}

/// The field's own edge strokes the radius the field's fill must fill, so the
/// two never disagree at a corner.
#[test]
fn the_trigger_edge_and_the_field_share_one_radius() {
    for scheme in [Scheme::Dark, Scheme::Light] {
        let t = *scheme.tokens();
        let field = corners(field_surface(t)(0.0, Status::Active).border.radius);
        for open in [0.0, 0.5, 1.0] {
            assert_eq!(
                corners(trigger_edge(t, open).radius),
                field,
                "{scheme:?}: the edge at open {open} does not match its fill"
            );
        }
    }
}

/// An unpicked segment fades one surface step in from nothing; the cell that
/// holds the selection takes no pointer surface of its own.
#[test]
fn a_segment_fades_the_pointer_surface_in_from_nothing() {
    for scheme in [Scheme::Dark, Scheme::Light] {
        let t = *scheme.tokens();
        let surface = segment_surface(t, true);
        assert_eq!(surface(0.0, Status::Active).background, None, "{scheme:?}");
        assert_eq!(
            surface(1.0, Status::Active).background,
            Some(Background::Color(t.surface_2)),
            "{scheme:?}"
        );
        assert_eq!(
            segment_surface(t, false)(1.0, Status::Active).background,
            None,
            "{scheme:?}: the selected cell painted a hover surface"
        );
    }
}
