#![allow(clippy::unwrap_used, clippy::expect_used)]

//! The effect picker's list is the CLI's grammar: every name the window offers
//! is one the parser takes, the effect the picker builds for it prints through
//! the CLI's own formatter and parses back to the same kind, and the picker's
//! index maps to it.

use gctl_proto::Rate;

use crate::gui::state::{EffectKind, build_effect, kind_of};
use crate::parse;

#[test]
fn every_offered_effect_is_one_the_cli_parses() {
    assert_eq!(parse::EFFECT_NAMES.len(), EffectKind::ALL.len());
    for (index, name) in parse::EFFECT_NAMES.iter().enumerate() {
        let kind = EffectKind::ALL[index];
        assert_eq!(kind.token(), *name);
        assert_eq!(kind.label(), parse::effect_label(name));
        assert_eq!(EffectKind::from_token(name), Some(kind));
        // The picker's index maps back through `from_token` to the same kind.
        assert_eq!(
            EffectKind::from_token(parse::EFFECT_NAMES[index]),
            Some(kind)
        );
        // The effect picking this name builds, printed by the CLI's own
        // formatter, is text the CLI parses to the same kind.
        let offered = build_effect(kind, Rate::DEFAULT, Vec::new());
        let text = parse::format_effect(&offered);
        assert_eq!(
            text.split_whitespace().next(),
            Some(*name),
            "{name:?} is printed as {text:?}"
        );
        let parsed = parse::effect(&text).unwrap_or_else(|error| panic!("{text:?}: {error}"));
        assert_eq!(kind_of(&parsed), kind, "{text:?} parses to another effect");
    }
}

#[test]
fn a_name_the_cli_rejects_is_not_offered() {
    assert!(parse::effect("no-such-effect").is_err());
    assert_eq!(EffectKind::from_token("no-such-effect"), None);
}
