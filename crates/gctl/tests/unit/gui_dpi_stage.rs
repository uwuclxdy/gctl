#![allow(clippy::unwrap_used, clippy::expect_used)]

//! The DPI entry's picked active stage: it stays the user's pick, clamped when
//! the entry's own list shrinks below it.

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
fn a_picked_stage_is_clamped_when_the_entry_list_shrinks_below_it() {
    let mut state = ready(snapshot());
    state.update(Msg::Edit(Edit::ActiveStage(3)));
    state.update(Msg::Edit(Edit::RemoveStage(2)));
    assert_eq!(state.active_stage(), Some(2));
    assert_eq!(
        state.plan(Section::Performance),
        Ok(vec![
            WriteOp::Dpi(stages(&[400, 800])),
            WriteOp::DpiStage(DpiStage::new(2).unwrap()),
        ])
    );
    // the clamp moves the pick itself: a list that grows again never revives it
    state.update(Msg::Edit(Edit::AddStage));
    assert_eq!(state.active_stage(), Some(2));
}
