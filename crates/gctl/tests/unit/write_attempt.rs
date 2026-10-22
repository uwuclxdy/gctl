#![allow(clippy::unwrap_used, clippy::expect_used)]

//! The CLI write path's per-attempt `reached`: the device's write count says
//! whether the failing report was handed to the transport, never the call.

use std::cell::{Cell, RefCell};
use std::time::Duration;

use gctl_proto::{Brightness, Device, Error, MODELS, Profile, Report, Transport, TransportError};

use super::*;

/// A fake mouse whose write reports fail while `fail_writes` is set and whose
/// DPI stage-count read answers `stages`.
struct Fake {
    sent: RefCell<Vec<Report>>,
    fail_writes: Cell<bool>,
    stages: Cell<u8>,
}

impl Fake {
    fn new(stages: u8) -> Self {
        Self {
            sent: RefCell::default(),
            fail_writes: Cell::new(false),
            stages: Cell::new(stages),
        }
    }

    fn writes(&self) -> usize {
        self.sent.borrow().iter().filter(|r| r[6] < 0x80).count()
    }
}

impl Transport for Fake {
    fn send(&self, report: &Report) -> Result<(), TransportError> {
        self.sent.borrow_mut().push(*report);
        if self.fail_writes.get() && report[6] < 0x80 {
            return Err("write refused by the fake".into());
        }
        Ok(())
    }

    fn receive(&self, report: &mut Report) -> Result<(), TransportError> {
        *report = [0u8; 65];
        if let Some(last) = self.sent.borrow().last()
            && (last[5], last[6]) == (0x01, 0x81)
        {
            report[6] = 0x81;
            report[8] = self.stages.get();
        }
        Ok(())
    }
}

fn device(fake: &Fake) -> Device<&Fake> {
    Device::new(fake, &MODELS[0]).with_io_delay(Duration::ZERO)
}

fn profile() -> Profile {
    Profile::new(2).unwrap()
}

#[test]
fn a_colour_list_the_mouse_refuses_before_its_report_reaches_nothing() {
    let fake = Fake::new(2);
    let attempt = dpi_colors(
        &device(&fake),
        profile(),
        &[
            gctl_proto::Rgb::default(),
            gctl_proto::Rgb::default(),
            gctl_proto::Rgb::default(),
        ],
        &mut Memory::default(),
    )
    .unwrap_err();
    assert!(
        matches!(attempt.error, Error::StageCountMismatch { .. }),
        "{:?}",
        attempt.error
    );
    assert!(!attempt.reached, "the refusal came before any report");
    assert_eq!(fake.writes(), 0);
}

#[test]
fn a_report_whose_send_fails_reaches() {
    let fake = Fake::new(2);
    fake.fail_writes.set(true);
    let attempt = brightness(
        &device(&fake),
        Brightness {
            wired: 10,
            wireless: 20,
        },
        &mut Memory::default(),
    )
    .unwrap_err();
    assert!(
        matches!(attempt.error, Error::Transport(_)),
        "{:?}",
        attempt.error
    );
    assert!(attempt.reached, "the failed report may have landed");
    assert_eq!(fake.writes(), 1);
}
