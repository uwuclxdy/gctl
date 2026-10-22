#![allow(clippy::unwrap_used, clippy::expect_used)]

//! An import answer with no re-read: `io::run_import` never ran under the lock
//! (no mouse, a failed open, a lock failure), so its check wrote nothing and
//! the answer leaves the snapshot alone.

use std::path::PathBuf;

use gctl_proto::{
    Battery, Debounce, DeviceState, Dpi, DpiStage, DpiStages, Firmware, LiftOff, MODELS,
    PollingRate, Profile, SleepDelay,
};

use super::*;
use crate::writes::WriteOp;

fn profile(id: u8) -> Profile {
    Profile::new(id).unwrap()
}

fn stages(values: &[u16]) -> DpiStages {
    DpiStages::new(
        values
            .iter()
            .map(|value| Dpi::new(*value).unwrap())
            .collect(),
    )
    .unwrap()
}

fn snapshot() -> Snapshot {
    Snapshot {
        model: &MODELS[0],
        battery: Battery {
            state: DeviceState::Awake,
            percent: Some(35),
        },
        firmware: Firmware([0, 3, 8, 0]),
        active: profile(2),
        profile: profile(2),
        polling_rate: PollingRate::Hz1000,
        lift_off: LiftOff::Mm1,
        sleep: SleepDelay::Seconds(120),
        brightness: None,
        scroll: None,
        dpi: stages(&[400, 800, 1600]),
        dpi_stage: DpiStage::new(2).unwrap(),
        dpi_colors: None,
        debounce: Debounce::new(10).unwrap(),
        effect: None,
        binds: Vec::new(),
        memory_error: None,
    }
}

fn ready(snap: Snapshot) -> State {
    let (mut state, _) = State::new();
    state.update(Msg::Loaded(Ok(snap)));
    state
}

#[test]
fn a_lock_failed_import_answer_changes_nothing_and_keeps_the_edit() {
    let mut state = ready(snapshot());
    state.update(Msg::Edit(Edit::PollingRate(PollingRate::Hz500)));
    state.update(Msg::Import(PathBuf::from("/p.toml")));
    state.update(Msg::ImportChecked(Ok(ImportCheck {
        note: None,
        device_wide: true,
        writes: vec![WriteOp::PollingRate(PollingRate::Hz500)],
    })));
    assert_eq!(state.update(Msg::ImportConfirm).len(), 1);
    // The lock was never taken: no report was sent and nothing was re-read.
    // The error is the one `io::with_lock` builds from `take_lock`'s failure.
    state.update(Msg::Imported {
        error: Some(
            "could not lock \"/home/c/.local/state/gctl/gctl.lock\": Permission denied (os error 13)"
                .to_owned(),
        ),
        written: Vec::new(),
        reached: false,
        snapshot: None,
        remembered: None,
    });
    assert_eq!(state.snapshot(), Some(&snapshot()), "nothing was written");
    assert!(
        state.pending(Section::Performance),
        "no write of this window landed, so the user's edit stays"
    );
}
