#![allow(clippy::unwrap_used, clippy::expect_used)]

//! The `Key::all` accessor the GUI's key picker uses.

use gctl_proto::Key;

#[test]
fn all_keys_lists_every_known_name_once() {
    let names: Vec<&str> = Key::all().map(|key| key.name()).collect();
    assert_eq!(names.len(), 90);

    // No name is repeated, so the picker cannot show a key twice.
    let mut sorted = names.clone();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(sorted.len(), 90);

    // Every listed key round-trips through `from_name`.
    for key in Key::all() {
        assert_eq!(Key::from_name(key.name()).ok(), Some(key));
    }

    // A few hand-picked names from mxw's table, not from this crate's.
    for name in ["KeyA", "F5", "ShiftLeft", "Enter", "ArrowUp", "NumpadAdd"] {
        assert!(
            names.contains(&name),
            "the firmware key set is missing {name}"
        );
    }
}
