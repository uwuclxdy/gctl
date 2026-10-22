#![allow(clippy::unwrap_used, clippy::expect_used)]

//! A failed write's fields count as this window's own until the next good read:
//! the failing report may have reached the mouse.

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
fn a_failed_import_owns_the_fields_its_check_carried_at_the_next_read() {
    let mut state = ready(snapshot());
    state.update(Msg::Edit(Edit::Debounce(0)));
    state.update(Msg::Import(PathBuf::from("/p.toml")));
    state.update(Msg::ImportChecked(Ok(ImportCheck {
        note: None,
        device_wide: false,
        writes: vec![WriteOp::Debounce(Debounce::new(4).unwrap())],
    })));
    // the failing debounce report may have reached the mouse
    state.update(Msg::Imported {
        error: Some("HID transport failed: gone".to_owned()),
        written: Vec::new(),
        reached: true,
        remembered: None,
        snapshot: Some(Ok(Snapshot {
            debounce: Debounce::new(4).unwrap(),
            ..snapshot()
        })),
    });
    assert!(state.changed().is_empty());
    assert!(!state.outside_change());
    // that read was good, so the next move is someone else's again
    state.update(Msg::Tick);
    state.update(Msg::Loaded(Ok(Snapshot {
        debounce: Debounce::new(7).unwrap(),
        ..snapshot()
    })));
    assert!(state.outside_change());
    assert_eq!(
        state.changed().to_vec(),
        vec!["debounce changed on the mouse to 7 ms; your edit is kept".to_owned()]
    );
}
