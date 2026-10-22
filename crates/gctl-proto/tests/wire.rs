#![allow(clippy::unwrap_used, clippy::expect_used)]

//! Byte-exact wire tests. Every expected report is copied by hand from mxw's
//! encoders (dkbednarczyk/mxw@5ae60fa) or from the rulings in the crate's
//! contract (battery `None` and clamp, sleep-delay bounds, write pauses, the
//! strict polling-rate read, the two-modifier bind), never derived from this crate.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, VecDeque};
use std::time::{Duration, Instant};

use gctl_proto::{
    Binding, Brightness, Button, Candidate, Debounce, Device, DeviceState, Dpi, DpiFn, DpiStage,
    DpiStages, Effect, Error, Key, KeyboardFn, LiftOff, Link, MODELS, MediaFn, Model, Modifiers,
    MouseFn, PollingRate, Profile, Rate, Report, Rgb, ScrollDirection, SleepDelay, Transport,
    TransportError, select,
};

/// A reply's routing key: the `([5] sub, [6] command)` pair of the request it answers.
type ReplyKey = (u8, u8);

/// Records every sent report; answers a receive with the next queued reply for the
/// `([5], [6])` pair of the last sent report, or an all-zero (stale) report.
/// Firmware and the DPI stage list share command `0x81`, sleep and lift-off share
/// `0x87`: keying by the command alone would answer a wrong-sub request.
#[derive(Default)]
struct Fake {
    sent: RefCell<Vec<Report>>,
    replies: RefCell<HashMap<ReplyKey, VecDeque<Report>>>,
    /// Records each write report (`[6]` below `0x80`), then fails its send.
    fail_writes: Cell<bool>,
}

impl Fake {
    fn reply(&self, key: ReplyKey, report: Report) {
        self.replies
            .borrow_mut()
            .entry(key)
            .or_default()
            .push_back(report);
    }

    fn sent(&self) -> Vec<Report> {
        self.sent.borrow().clone()
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
        let key = self.sent.borrow().last().map_or((0, 0), |r| (r[5], r[6]));
        *report = self
            .replies
            .borrow_mut()
            .get_mut(&key)
            .and_then(VecDeque::pop_front)
            .unwrap_or([0; 65]);
        Ok(())
    }
}

fn model() -> &'static Model {
    Model::from_pid(0x2022).unwrap()
}

fn device(fake: &Fake) -> Device<&Fake> {
    Device::new(fake, model()).with_io_delay(Duration::ZERO)
}

/// A report with byte 3 = 0x02 (every request carries it) plus the given bytes.
fn req(bytes: &[(usize, u8)]) -> Report {
    let mut r = [0u8; 65];
    r[3] = 0x02;
    for &(i, b) in bytes {
        r[i] = b;
    }
    r
}

/// A reply report: only the given bytes set.
fn rep(bytes: &[(usize, u8)]) -> Report {
    let mut r = [0u8; 65];
    for &(i, b) in bytes {
        r[i] = b;
    }
    r
}

fn p(id: u8) -> Profile {
    Profile::new(id).unwrap()
}

fn rgb(hex: &str) -> Rgb {
    hex.parse().unwrap()
}

fn rate(r: u8) -> Rate {
    Rate::new(r).unwrap()
}

// ---------------------------------------------------------------- reads

#[test]
fn profile_read_sends_request_and_decodes_reply() {
    let fake = Fake::default();
    fake.reply((0, 0x85), rep(&[(6, 0x85), (7, 3)]));
    assert_eq!(device(&fake).profile().unwrap(), p(3));
    assert_eq!(fake.sent(), vec![req(&[(4, 0x01), (6, 0x85)])]);
}

#[test]
fn read_polls_past_stale_replies() {
    let fake = Fake::default();
    fake.reply((0, 0x85), rep(&[(6, 0x00)]));
    fake.reply((0, 0x85), rep(&[(6, 0x00)]));
    fake.reply((0, 0x85), rep(&[(6, 0x85), (7, 2)]));
    assert_eq!(device(&fake).profile().unwrap(), p(2));
    assert_eq!(fake.sent().len(), 1, "polling re-reads, it never re-sends");
}

#[test]
fn read_polls_five_times_before_giving_up() {
    let fake = Fake::default();
    for _ in 0..4 {
        fake.reply((0, 0x85), rep(&[(6, 0x00)]));
    }
    fake.reply((0, 0x85), rep(&[(6, 0x85), (7, 2)]));
    assert_eq!(
        device(&fake).profile().unwrap(),
        p(2),
        "the fifth poll still counts"
    );
}

#[test]
fn read_gives_up_after_five_stale_replies() {
    let fake = Fake::default();
    for _ in 0..5 {
        fake.reply((0, 0x85), rep(&[(6, 0x00)]));
    }
    fake.reply((0, 0x85), rep(&[(6, 0x85), (7, 2)]));
    let err = device(&fake).profile().unwrap_err();
    assert!(
        matches!(err, Error::NoResponse { command: 0x85 }),
        "{err:?}"
    );
}

#[test]
fn firmware_read() {
    let fake = Fake::default();
    fake.reply(
        (0, 0x81),
        rep(&[(6, 0x81), (7, 0), (8, 3), (9, 8), (10, 0)]),
    );
    let fw = device(&fake).firmware().unwrap();
    assert_eq!(fw.to_string(), "0.3.8.0");
    assert_eq!(fake.sent(), vec![req(&[(4, 0x03), (6, 0x81)])]);
}

#[test]
fn battery_read_decodes_state_and_percent() {
    let fake = Fake::default();
    fake.reply((0, 0x83), rep(&[(1, 0xA1), (6, 0x83), (8, 35)]));
    fake.reply((0, 0x83), rep(&[(1, 0xA4), (6, 0x83), (8, 50)]));
    let dev = device(&fake);

    let b = dev.battery().unwrap();
    assert_eq!((b.state, b.percent), (DeviceState::Awake, Some(35)));
    assert_eq!(dev.battery().unwrap().state, DeviceState::Asleep);
    assert_eq!(fake.sent()[0], req(&[(4, 0x02), (6, 0x83)]));
}

#[test]
fn battery_raw_zero_after_power_on_is_unknown() {
    let fake = Fake::default();
    fake.reply((0, 0x83), rep(&[(1, 0xA1), (6, 0x83), (8, 0)]));
    fake.reply((0, 0x83), rep(&[(1, 0xA1), (6, 0x83), (8, 1)]));
    let dev = device(&fake);
    assert_eq!(dev.battery().unwrap().percent, None, "raw 0");
    assert_eq!(dev.battery().unwrap().percent, Some(1), "raw 1");
}

#[test]
fn battery_above_100_clamps_to_100() {
    let fake = Fake::default();
    fake.reply((0, 0x83), rep(&[(1, 0xA1), (6, 0x83), (8, 100)]));
    fake.reply((0, 0x83), rep(&[(1, 0xA1), (6, 0x83), (8, 101)]));
    fake.reply((0, 0x83), rep(&[(1, 0xA1), (6, 0x83), (8, 255)]));
    let dev = device(&fake);
    assert_eq!(dev.battery().unwrap().percent, Some(100), "raw 100");
    assert_eq!(dev.battery().unwrap().percent, Some(100), "raw 101");
    assert_eq!(dev.battery().unwrap().percent, Some(100), "raw 255");
}

#[test]
fn status_states() {
    let fake = Fake::default();
    fake.reply((0, 0x83), rep(&[(1, 0xA0), (6, 0x83)]));
    fake.reply((0, 0x83), rep(&[(1, 0x55), (6, 0x83)]));
    fake.reply((0, 0x83), rep(&[(1, 0xA1), (6, 0x00)]));
    let dev = device(&fake);
    assert_eq!(dev.status().unwrap(), DeviceState::WakingUp);
    assert_eq!(dev.status().unwrap(), DeviceState::Unknown(0x55));
    assert_eq!(dev.status().unwrap(), DeviceState::Invalid);
}

#[test]
fn sleep_delay_read() {
    let fake = Fake::default();
    fake.reply((0, 0x87), rep(&[(6, 0x87), (7, 0x01), (8, 0x2C)]));
    fake.reply((0, 0x87), rep(&[(6, 0x87), (7, 0xFF), (8, 0xFF)]));
    let dev = device(&fake);
    assert_eq!(dev.sleep_delay().unwrap(), SleepDelay::Seconds(300));
    assert_eq!(dev.sleep_delay().unwrap(), SleepDelay::Never);
    assert_eq!(fake.sent()[0], req(&[(4, 0x02), (6, 0x87)]));
}

#[test]
fn lift_off_read() {
    let fake = Fake::default();
    fake.reply((1, 0x87), rep(&[(6, 0x87), (7, 0)]));
    fake.reply((1, 0x87), rep(&[(6, 0x87), (7, 1)]));
    fake.reply((1, 0x87), rep(&[(6, 0x87), (7, 2)]));
    let dev = device(&fake);
    assert_eq!(dev.lift_off().unwrap(), LiftOff::Mm1);
    assert_eq!(dev.lift_off().unwrap(), LiftOff::Mm2);
    assert!(matches!(dev.lift_off(), Err(Error::Implausible { .. })));
    assert_eq!(fake.sent()[0], req(&[(4, 0x01), (5, 0x01), (6, 0x87)]));
}

#[test]
fn polling_rate_read() {
    let fake = Fake::default();
    fake.reply((1, 0x80), rep(&[(6, 0x80), (7, 8)]));
    fake.reply((1, 0x80), rep(&[(6, 0x80), (7, 1)]));
    fake.reply((1, 0x80), rep(&[(6, 0x80), (7, 3)]));
    let dev = device(&fake);
    assert_eq!(dev.polling_rate().unwrap(), PollingRate::Hz125);
    assert_eq!(dev.polling_rate().unwrap(), PollingRate::Hz1000);
    assert!(matches!(dev.polling_rate(), Err(Error::Implausible { .. })));
    assert_eq!(fake.sent()[0], req(&[(4, 0x01), (5, 0x01), (6, 0x80)]));
}

#[test]
fn debounce_read_is_per_profile() {
    let fake = Fake::default();
    fake.reply((0, 0x88), rep(&[(6, 0x88), (8, 10)]));
    fake.reply((0, 0x88), rep(&[(6, 0x88), (8, 17)]));
    let dev = device(&fake);
    assert_eq!(dev.debounce(p(2)).unwrap(), Debounce::new(10).unwrap());
    assert!(matches!(dev.debounce(p(2)), Err(Error::Implausible { .. })));
    assert_eq!(fake.sent()[0], req(&[(4, 0x02), (6, 0x88), (7, 2)]));
}

#[test]
fn dpi_read() {
    let fake = Fake::default();
    fake.reply((1, 0x82), rep(&[(6, 0x82), (8, 2)]));
    fake.reply(
        (1, 0x81),
        rep(&[
            (6, 0x81),
            (8, 3),
            (9, 0x01),
            (10, 0x90),
            (13, 0x03),
            (14, 0x20),
            (17, 0x06),
            (18, 0x40),
        ]),
    );
    let cfg = device(&fake).dpi(p(1)).unwrap();
    let dpis: Vec<u16> = cfg.stages.as_slice().iter().map(|d| d.get()).collect();
    assert_eq!(dpis, vec![400, 800, 1600]);
    assert_eq!(cfg.active.get(), 2);

    assert_eq!(
        fake.sent(),
        vec![
            req(&[(4, 0x02), (5, 0x01), (6, 0x82), (7, 1)]),
            req(&[(4, 26), (5, 0x01), (6, 0x81), (7, 1)]),
        ],
        "active stage first, then the stage list, as mxw reads them"
    );
}

#[test]
fn profile_read_rejects_an_implausible_id() {
    let fake = Fake::default();
    fake.reply((0, 0x85), rep(&[(6, 0x85), (7, 0)]));
    fake.reply((0, 0x85), rep(&[(6, 0x85), (7, 4)]));
    let dev = device(&fake);
    let zero = dev.profile().unwrap_err();
    assert!(
        matches!(zero, Error::Implausible { value: 0, .. }),
        "a device value is never a caller error: {zero:?}"
    );
    let four = dev.profile().unwrap_err();
    assert!(
        matches!(four, Error::Implausible { value: 4, .. }),
        "{four:?}"
    );
}

#[test]
fn dpi_read_rejects_an_implausible_stage_value() {
    let fake = Fake::default();
    fake.reply((1, 0x82), rep(&[(6, 0x82), (8, 1)]));
    fake.reply((1, 0x81), rep(&[(6, 0x81), (8, 1), (9, 0x00), (10, 0x32)]));
    let err = device(&fake).dpi(p(1)).unwrap_err();
    assert!(
        matches!(err, Error::Implausible { value: 50, .. }),
        "{err:?}"
    );
}

#[test]
fn sleep_delay_read_rejects_a_raw_zero() {
    let fake = Fake::default();
    fake.reply((0, 0x87), rep(&[(6, 0x87), (7, 0), (8, 0)]));
    let err = device(&fake).sleep_delay().unwrap_err();
    assert!(
        matches!(err, Error::Implausible { value: 0, .. }),
        "raw 0 is neither a writable delay nor never: {err:?}"
    );
}

/// Fails every send with the error text `boom`.
struct Broken;

impl Transport for Broken {
    fn send(&self, _report: &Report) -> Result<(), TransportError> {
        Err("boom".into())
    }

    fn receive(&self, _report: &mut Report) -> Result<(), TransportError> {
        Err("boom".into())
    }
}

#[test]
fn transport_error_names_its_cause_once() {
    use std::error::Error as _;
    let err = Device::new(Broken, model())
        .with_io_delay(Duration::ZERO)
        .profile()
        .unwrap_err();
    assert_eq!(err.to_string(), "HID transport failed");
    assert_eq!(
        err.source().map(ToString::to_string),
        Some("boom".to_owned())
    );
}

#[test]
fn dpi_read_rejects_implausible_counts() {
    let fake = Fake::default();
    fake.reply((1, 0x82), rep(&[(6, 0x82), (8, 1)]));
    fake.reply((1, 0x81), rep(&[(6, 0x81), (8, 0)]));
    assert!(matches!(
        device(&fake).dpi(p(1)),
        Err(Error::Implausible { .. })
    ));

    let fake = Fake::default();
    fake.reply((1, 0x82), rep(&[(6, 0x82), (8, 4)]));
    fake.reply((1, 0x81), rep(&[(6, 0x81), (8, 3), (9, 0x01), (10, 0x90)]));
    assert!(matches!(
        device(&fake).dpi(p(1)),
        Err(Error::Implausible { .. })
    ));
}

// ---------------------------------------------------------------- writes

#[test]
fn set_profile() {
    let fake = Fake::default();
    device(&fake).set_profile(p(2)).unwrap();
    assert_eq!(fake.sent(), vec![req(&[(4, 0x01), (6, 0x05), (7, 2)])]);
}

#[test]
fn set_sleep_delay() {
    let fake = Fake::default();
    let dev = device(&fake);
    dev.set_sleep_delay(SleepDelay::Seconds(150)).unwrap();
    dev.set_sleep_delay(SleepDelay::Never).unwrap();
    assert_eq!(
        fake.sent(),
        vec![
            req(&[(4, 0x02), (6, 0x07), (7, 0x00), (8, 0x96)]),
            req(&[(4, 0x02), (6, 0x07), (7, 0xFF), (8, 0xFF)]),
        ]
    );
}

#[test]
fn set_sleep_delay_rejects_zero_and_the_never_sentinel() {
    let fake = Fake::default();
    let dev = device(&fake);
    let zero = dev.set_sleep_delay(SleepDelay::Seconds(0)).unwrap_err();
    assert!(
        matches!(
            zero,
            Error::OutOfRange {
                value: 0,
                min: 1,
                max: 0xFFFE,
                ..
            }
        ),
        "{zero:?}"
    );
    let sentinel = dev
        .set_sleep_delay(SleepDelay::Seconds(0xFFFF))
        .unwrap_err();
    assert!(
        matches!(
            sentinel,
            Error::OutOfRange {
                value: 0xFFFF,
                min: 1,
                max: 0xFFFE,
                ..
            }
        ),
        "{sentinel:?}"
    );
    assert!(
        fake.sent().is_empty(),
        "nothing written for a rejected delay"
    );
}

#[test]
fn set_sleep_delay_writes_the_range_ends() {
    let fake = Fake::default();
    let dev = device(&fake);
    dev.set_sleep_delay(SleepDelay::Seconds(1)).unwrap();
    dev.set_sleep_delay(SleepDelay::Seconds(0xFFFE)).unwrap();
    assert_eq!(
        fake.sent(),
        vec![
            req(&[(4, 0x02), (6, 0x07), (7, 0x00), (8, 0x01)]),
            req(&[(4, 0x02), (6, 0x07), (7, 0xFF), (8, 0xFE)]),
        ]
    );
}

#[test]
fn set_lift_off() {
    let fake = Fake::default();
    let dev = device(&fake);
    dev.set_lift_off(LiftOff::Mm1).unwrap();
    dev.set_lift_off(LiftOff::Mm2).unwrap();
    assert_eq!(
        fake.sent(),
        vec![
            req(&[(4, 0x01), (5, 0x01), (6, 0x07), (7, 0)]),
            req(&[(4, 0x01), (5, 0x01), (6, 0x07), (7, 1)]),
        ]
    );
}

#[test]
fn set_polling_rate() {
    let fake = Fake::default();
    device(&fake).set_polling_rate(PollingRate::Hz500).unwrap();
    assert_eq!(fake.sent(), vec![req(&[(4, 0x01), (5, 0x01), (7, 2)])]);
}

#[test]
fn set_debounce() {
    let fake = Fake::default();
    device(&fake)
        .set_debounce(p(1), Debounce::new(0).unwrap())
        .unwrap();
    assert_eq!(
        fake.sent(),
        vec![req(&[(4, 0x01), (6, 0x08), (7, 1), (8, 0)])]
    );
}

#[test]
fn set_dpi_stages_resets_active_stage_first() {
    let fake = Fake::default();
    let stages = DpiStages::new(vec![Dpi::new(400).unwrap(), Dpi::new(800).unwrap()]).unwrap();
    device(&fake).set_dpi_stages(p(1), &stages).unwrap();
    assert_eq!(
        fake.sent(),
        vec![
            req(&[(4, 0x02), (5, 0x01), (6, 0x02), (7, 1), (8, 1)]),
            req(&[
                (4, 10),
                (5, 0x01),
                (6, 0x01),
                (7, 1),
                (8, 2),
                (9, 0x01),
                (10, 0x90),
                (11, 0x01),
                (12, 0x90),
                (13, 0x03),
                (14, 0x20),
                (15, 0x03),
                (16, 0x20),
            ]),
        ]
    );
}

fn stage_count_reply(fake: &Fake, count: u8) {
    fake.reply((1, 0x81), rep(&[(6, 0x81), (8, count)]));
}

#[test]
fn set_dpi_stage_checks_the_stage_count() {
    let fake = Fake::default();
    stage_count_reply(&fake, 3);
    device(&fake)
        .set_dpi_stage(p(1), DpiStage::new(3).unwrap())
        .unwrap();
    assert_eq!(
        fake.sent(),
        vec![
            req(&[(4, 26), (5, 0x01), (6, 0x81), (7, 1)]),
            req(&[(4, 0x02), (5, 0x01), (6, 0x02), (7, 1), (8, 3)]),
        ]
    );

    let fake = Fake::default();
    stage_count_reply(&fake, 2);
    let err = device(&fake)
        .set_dpi_stage(p(1), DpiStage::new(3).unwrap())
        .unwrap_err();
    assert!(
        matches!(
            err,
            Error::OutOfRange {
                value: 3,
                min: 1,
                max: 2,
                ..
            }
        ),
        "{err:?}"
    );
    assert_eq!(
        fake.sent(),
        vec![req(&[(4, 26), (5, 0x01), (6, 0x81), (7, 1)])],
        "only the count read, no stage write"
    );
}

#[test]
fn set_dpi_colors_needs_one_colour_per_stage() {
    let fake = Fake::default();
    stage_count_reply(&fake, 2);
    device(&fake)
        .set_dpi_colors(p(1), &[rgb("ff0000"), rgb("00ff00")])
        .unwrap();
    assert_eq!(
        fake.sent(),
        vec![
            req(&[(4, 26), (5, 0x01), (6, 0x81), (7, 1)]),
            req(&[(4, 7), (5, 0x02), (6, 0x01), (7, 1), (8, 0xFF), (12, 0xFF)]),
        ]
    );

    let fake = Fake::default();
    stage_count_reply(&fake, 3);
    let err = device(&fake)
        .set_dpi_colors(p(1), &[rgb("ff0000"), rgb("00ff00")])
        .unwrap_err();
    assert!(
        matches!(
            err,
            Error::StageCountMismatch {
                profile: 1,
                stages: 3,
                given: 2
            }
        ),
        "{err:?}"
    );
    assert_eq!(fake.sent().len(), 1, "only the count read, no colour write");
}

#[test]
fn set_brightness_writes_wired_then_wireless() {
    let fake = Fake::default();
    device(&fake)
        .set_brightness(Brightness {
            wired: 200,
            wireless: 50,
        })
        .unwrap();
    assert_eq!(
        fake.sent(),
        vec![
            req(&[(4, 0x02), (5, 0x02), (6, 0x02), (7, 0x01), (8, 200)]),
            req(&[(4, 0x02), (5, 0x02), (6, 0x02), (7, 0x00), (8, 50)]),
        ]
    );
}

// ---------------------------------------------------------------- pauses

const PAUSE: Duration = Duration::from_millis(20);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Io {
    Send,
    Receive,
}

/// Records when each report was sent or received. A receive first answers with
/// the next scripted reply, then with an all-zero report echoing the last sent
/// `[6]`, which a read accepts on that poll and a bind reads as acknowledged.
#[derive(Default)]
struct Clocked {
    events: RefCell<Vec<(Io, Instant)>>,
    last_command: Cell<u8>,
    script: RefCell<VecDeque<Report>>,
}

impl Clocked {
    fn scripted(replies: &[Report]) -> Self {
        let clocked = Self::default();
        clocked.script.borrow_mut().extend(replies.iter().copied());
        clocked
    }

    fn times(&self, io: Io) -> Vec<Instant> {
        self.events
            .borrow()
            .iter()
            .filter(|(kind, _)| *kind == io)
            .map(|(_, at)| *at)
            .collect()
    }
}

impl Transport for Clocked {
    fn send(&self, report: &Report) -> Result<(), TransportError> {
        self.last_command.set(report[6]);
        self.events.borrow_mut().push((Io::Send, Instant::now()));
        Ok(())
    }

    fn receive(&self, report: &mut Report) -> Result<(), TransportError> {
        self.events.borrow_mut().push((Io::Receive, Instant::now()));
        *report = self.script.borrow_mut().pop_front().unwrap_or_else(|| {
            let mut echo = [0; 65];
            echo[6] = self.last_command.get();
            echo
        });
        Ok(())
    }
}

fn paced(clocked: &Clocked) -> Device<&Clocked> {
    Device::new(clocked, model()).with_io_delay(PAUSE)
}

/// Asserts the exact send/receive sequence, each event at least [`PAUSE`]
/// after the event before it.
fn assert_events_paced(clocked: &Clocked, expected: &[Io]) {
    let events = clocked.events.borrow();
    let kinds: Vec<Io> = events.iter().map(|(io, _)| *io).collect();
    assert_eq!(kinds, expected, "event sequence");
    for (i, pair) in events.windows(2).enumerate() {
        let gap = pair[1].1 - pair[0].1;
        assert!(
            gap >= PAUSE,
            "event {} ({:?}) came {gap:?} after event {i}, under {PAUSE:?}",
            i + 1,
            pair[1].0
        );
    }
}

/// A stale reply: `[6]` matches no read command.
const STALE: Report = [0; 65];

#[test]
fn read_waits_before_every_poll() {
    let clocked = Clocked::scripted(&[STALE, STALE]);
    paced(&clocked).firmware().unwrap();
    assert_events_paced(&clocked, &[Io::Send, Io::Receive, Io::Receive, Io::Receive]);
}

#[test]
fn status_waits_before_its_one_receive() {
    let clocked = Clocked::default();
    paced(&clocked).status().unwrap();
    assert_events_paced(&clocked, &[Io::Send, Io::Receive]);
}

#[test]
fn bind_waits_before_reading_the_acknowledgement() {
    let clocked = Clocked::default();
    paced(&clocked)
        .bind(p(1), Button::Back, Binding::None)
        .unwrap();
    assert_events_paced(&clocked, &[Io::Send, Io::Receive]);
}

#[test]
fn bind_resend_waits_after_the_resend_request() {
    let clocked = Clocked::scripted(&[rep(&[(0, 0xA2)])]);
    paced(&clocked)
        .bind(p(1), Button::Back, Binding::None)
        .unwrap();
    assert_events_paced(&clocked, &[Io::Send, Io::Receive, Io::Send, Io::Receive]);
}

#[test]
fn bind_waits_out_its_attempts_once_the_mouse_reports_asleep() {
    let clocked = Clocked::scripted(&[rep(&[(0, 0xA4)])]);
    let err = paced(&clocked)
        .bind(p(1), Button::Back, Binding::None)
        .unwrap_err();
    let done = Instant::now();
    assert!(
        matches!(err, Error::BindNotAcknowledged { attempts: 3 }),
        "{err:?}"
    );
    // mxw's path sleeps 4 times: before each of the 3 attempts, and once after
    // the receive that saw 0xA4.
    let waited = done - clocked.times(Io::Send)[0];
    assert!(
        waited >= 4 * PAUSE,
        "failed {waited:?} after the send, before the 3 attempts were spent"
    );
}

/// Asserts `count` sends, each at least [`PAUSE`] after the one before it.
fn assert_paused(clocked: &Clocked, count: usize) {
    let sends = clocked.times(Io::Send);
    assert_eq!(sends.len(), count, "send count");
    for (i, pair) in sends.windows(2).enumerate() {
        let gap = pair[1] - pair[0];
        assert!(
            gap >= PAUSE,
            "send {} came {gap:?} after send {i}, under {PAUSE:?}",
            i + 1
        );
    }
}

#[test]
fn set_brightness_pauses_between_its_two_reports() {
    let clocked = Clocked::default();
    paced(&clocked)
        .set_brightness(Brightness {
            wired: 200,
            wireless: 50,
        })
        .unwrap();
    assert_paused(&clocked, 2);
}

#[test]
fn set_dpi_stages_pauses_after_resetting_the_active_stage() {
    let clocked = Clocked::default();
    let stages = DpiStages::new(vec![Dpi::new(400).unwrap(), Dpi::new(800).unwrap()]).unwrap();
    paced(&clocked).set_dpi_stages(p(1), &stages).unwrap();
    assert_paused(&clocked, 2);
}

#[test]
fn set_scroll_pauses_between_its_six_binds() {
    let clocked = Clocked::default();
    paced(&clocked).set_scroll(ScrollDirection::Invert).unwrap();
    assert_paused(&clocked, 6);
}

fn effect_bytes(effect: &Effect) -> Report {
    let fake = Fake::default();
    device(&fake).set_effect(p(2), effect).unwrap();
    let sent = fake.sent();
    assert_eq!(sent.len(), 1);
    sent[0]
}

/// The bytes every effect report shares.
fn effect_req(bytes: &[(usize, u8)]) -> Report {
    let mut all = vec![(5, 0x02), (7, 2), (8, 0xFF)];
    all.extend_from_slice(bytes);
    req(&all)
}

#[test]
fn set_effect_encodings() {
    assert_eq!(effect_bytes(&Effect::Off), effect_req(&[(4, 5), (9, 0)]));
    assert_eq!(
        effect_bytes(&Effect::Glorious { rate: rate(40) }),
        effect_req(&[(4, 5), (9, 1), (11, 13)])
    );
    assert_eq!(
        effect_bytes(&Effect::Cycle { rate: rate(100) }),
        effect_req(&[(4, 5), (9, 2), (11, 1), (12, 0xFF)])
    );
    assert_eq!(
        effect_bytes(&Effect::Pulse {
            rate: rate(40),
            colors: vec![rgb("ff0000"), rgb("0000ff")],
        }),
        effect_req(&[(4, 11), (9, 3), (11, 13), (12, 0xFF), (17, 0xFF)])
    );
    assert_eq!(
        effect_bytes(&Effect::Solid {
            color: rgb("00ff00")
        }),
        effect_req(&[(4, 8), (9, 4), (13, 0xFF)])
    );
    assert_eq!(
        effect_bytes(&Effect::PulseOne {
            rate: rate(0),
            color: rgb("102030"),
        }),
        effect_req(&[(4, 8), (9, 5), (11, 21), (12, 0x10), (13, 0x20), (14, 0x30)])
    );
    assert_eq!(
        effect_bytes(&Effect::Tail { rate: rate(55) }),
        effect_req(&[(4, 5), (9, 6), (11, 10)])
    );
    assert_eq!(
        effect_bytes(&Effect::Rave {
            rate: rate(40),
            colors: vec![rgb("ff0000"), rgb("00ff00")],
        }),
        effect_req(&[(4, 11), (9, 7), (11, 130), (12, 0xFF), (16, 0xFF)])
    );
    assert_eq!(
        effect_bytes(&Effect::Wave { rate: rate(0) }),
        effect_req(&[(4, 5), (9, 8), (11, 210)])
    );
}

#[test]
fn effect_check_bounds_the_colour_counts() {
    let pulse = |n: usize| Effect::Pulse {
        rate: rate(40),
        colors: vec![rgb("ff0000"); n],
    };
    let rave = |n: usize| Effect::Rave {
        rate: rate(40),
        colors: vec![rgb("ff0000"); n],
    };
    assert_out_of_range(pulse(1).check(), (1, 2, 6));
    assert_eq!(
        pulse(1).check().unwrap_err().to_string(),
        "colour count 1 is out of range 2..=6"
    );
    assert!(pulse(2).check().is_ok());
    assert!(pulse(6).check().is_ok());
    assert_out_of_range(pulse(7).check(), (7, 2, 6));
    assert_out_of_range(rave(0).check(), (0, 1, 2));
    assert!(rave(1).check().is_ok());
    assert!(rave(2).check().is_ok());
    assert_out_of_range(rave(3).check(), (3, 1, 2));
    assert!(Effect::Off.check().is_ok());
}

#[test]
fn set_effect_rejects_bad_colour_counts() {
    let fake = Fake::default();
    let dev = device(&fake);
    let one = vec![rgb("ff0000")];
    let seven = vec![rgb("ff0000"); 7];
    assert!(matches!(
        dev.set_effect(
            p(1),
            &Effect::Pulse {
                rate: rate(40),
                colors: one
            }
        ),
        Err(Error::OutOfRange { .. })
    ));
    assert!(matches!(
        dev.set_effect(
            p(1),
            &Effect::Pulse {
                rate: rate(40),
                colors: seven
            }
        ),
        Err(Error::OutOfRange { .. })
    ));
    assert!(matches!(
        dev.set_effect(
            p(1),
            &Effect::Rave {
                rate: rate(40),
                colors: vec![rgb("ff0000"); 3]
            }
        ),
        Err(Error::OutOfRange { .. })
    ));
    assert!(fake.sent().is_empty());
}

// ---------------------------------------------------------------- binds

fn bind_bytes(button: Button, binding: Binding) -> Report {
    let fake = Fake::default();
    device(&fake).bind(p(1), button, binding).unwrap();
    fake.sent()[0]
}

/// The bytes every bind report shares, for profile 1.
fn bind_req(button: u8, payload: &[(usize, u8)]) -> Report {
    let mut all = vec![(4, 0x09), (5, 0x03), (7, 1), (8, button)];
    all.extend_from_slice(payload);
    req(&all)
}

#[test]
fn bind_button_ids() {
    let ids = [
        (Button::Left, 1),
        (Button::Right, 2),
        (Button::Scroll, 3),
        (Button::Back, 4),
        (Button::Forward, 5),
        (Button::ScrollUp, 16),
        (Button::ScrollDown, 17),
        (Button::Dpi, 20),
    ];
    for (button, id) in ids {
        assert_eq!(
            bind_bytes(button, Binding::None),
            bind_req(id, &[]),
            "{button:?}"
        );
    }
}

// one row per variant, from mxw `src/config/bind/mod.rs` `set_mouse`,
// `set_keyboard`, `set_dpi` and `set_media`.

#[test]
fn bind_mouse_functions() {
    let cases = [
        (MouseFn::Left, [1, 1, 1]),
        (MouseFn::Right, [1, 1, 2]),
        (MouseFn::Scroll, [1, 1, 3]),
        (MouseFn::Back, [1, 1, 4]),
        (MouseFn::Forward, [1, 1, 5]),
        (MouseFn::ScrollUp, [1, 1, 16]),
        (MouseFn::ScrollDown, [1, 1, 17]),
        (MouseFn::ProfileCycleUp, [8, 1, 4]),
        (MouseFn::ProfileCycleDown, [8, 1, 3]),
        (MouseFn::BatteryStatus, [12, 1, 1]),
    ];
    for (f, [a, b, c]) in cases {
        assert_eq!(
            bind_bytes(Button::Back, Binding::Mouse(f)),
            bind_req(4, &[(10, a), (11, b), (12, c)]),
            "{f:?}"
        );
    }
}

#[test]
fn bind_keyboard_functions() {
    let cases = [
        (KeyboardFn::ProfileCycleUp, 1),
        (KeyboardFn::ProfileCycleDown, 2),
        (KeyboardFn::LayerCycleUp, 3),
        (KeyboardFn::LayerCycleDown, 4),
    ];
    for (f, id) in cases {
        assert_eq!(
            bind_bytes(Button::Forward, Binding::Keyboard(f)),
            bind_req(5, &[(10, 5), (11, 2), (12, id), (13, 0x0F)]),
            "{f:?}"
        );
    }
}

#[test]
fn bind_dpi_functions() {
    let cases = [
        (DpiFn::StageUp, 1),
        (DpiFn::StageDown, 2),
        (DpiFn::CycleUp, 6),
        (DpiFn::CycleDown, 7),
    ];
    for (f, id) in cases {
        assert_eq!(
            bind_bytes(Button::Dpi, Binding::Dpi(f)),
            bind_req(20, &[(10, 7), (11, 1), (12, id)]),
            "{f:?}"
        );
    }
}

#[test]
fn bind_media_functions() {
    let cases = [
        (MediaFn::Player, [1, 131]),
        (MediaFn::PlayPause, [0, 205]),
        (MediaFn::Next, [0, 181]),
        (MediaFn::Previous, [0, 182]),
        (MediaFn::Stop, [0, 183]),
        (MediaFn::Mute, [0, 226]),
        (MediaFn::VolumeUp, [0, 233]),
        (MediaFn::VolumeDown, [0, 234]),
    ];
    for (f, [b1, b2]) in cases {
        assert_eq!(
            bind_bytes(Button::Back, Binding::Media(f)),
            bind_req(4, &[(10, 5), (11, 2), (12, b1), (13, b2)]),
            "{f:?}"
        );
    }
}

#[test]
fn bind_key_with_modifiers() {
    let a = Key::from_name("KeyA").unwrap();
    assert_eq!(a.usage(), 4);
    assert_eq!(
        bind_bytes(
            Button::Back,
            Binding::Key {
                key: a,
                modifiers: Modifiers::CTRL_LEFT | Modifiers::SHIFT_LEFT,
            }
        ),
        bind_req(4, &[(10, 4), (11, 2), (12, 3), (13, 4)])
    );
}

#[test]
fn bind_modifier_key_sets_its_bit_and_no_scan_code() {
    let shift = Key::from_name("ShiftLeft").unwrap();
    assert_eq!(shift.usage(), 0xE1);
    assert_eq!(shift.modifier(), Some(Modifiers::SHIFT_LEFT));
    assert_eq!(
        bind_bytes(
            Button::Back,
            Binding::Key {
                key: shift,
                modifiers: Modifiers::CTRL_LEFT,
            }
        ),
        bind_req(4, &[(10, 4), (11, 2), (12, 3)])
    );
}

#[test]
fn key_names_follow_hid_usage() {
    assert_eq!(Key::from_name("AltLeft").unwrap().usage(), 0xE2);
    assert_eq!(Key::from_name("MetaRight").unwrap().usage(), 0xE7);
    assert_eq!(Key::from_name("F12").unwrap().usage(), 0x45);
    assert_eq!(Key::from_name("KeyA").unwrap().name(), "KeyA");
    assert!(matches!(
        Key::from_name("ShiftRight"),
        Err(Error::UnknownKey(name)) if name == "ShiftRight"
    ));
    assert!(matches!(
        Key::from_name("AltRight"),
        Err(Error::UnknownKey(name)) if name == "AltRight"
    ));
    assert!(matches!(
        Key::from_name("ControlRight"),
        Err(Error::UnknownKey(name)) if name == "ControlRight"
    ));
}

/// mxw `src/util/key.rs` `RAW_KEYS` as `(code, scan_code)`, with mxw's
/// `ShiftRight` (225) and `AltRight` (226) renamed to the HID usage names.
const MXW_KEYS: [(&str, u8); 90] = [
    ("KeyA", 4),
    ("KeyB", 5),
    ("KeyC", 6),
    ("KeyD", 7),
    ("KeyE", 8),
    ("KeyF", 9),
    ("KeyG", 10),
    ("KeyH", 11),
    ("KeyI", 12),
    ("KeyJ", 13),
    ("KeyK", 14),
    ("KeyL", 15),
    ("KeyM", 16),
    ("KeyN", 17),
    ("KeyO", 18),
    ("KeyP", 19),
    ("KeyQ", 20),
    ("KeyR", 21),
    ("KeyS", 22),
    ("KeyT", 23),
    ("KeyU", 24),
    ("KeyV", 25),
    ("KeyW", 26),
    ("KeyX", 27),
    ("KeyY", 28),
    ("KeyZ", 29),
    ("Digit1", 30),
    ("Digit2", 31),
    ("Digit3", 32),
    ("Digit4", 33),
    ("Digit5", 34),
    ("Digit6", 35),
    ("Digit7", 36),
    ("Digit8", 37),
    ("Digit9", 38),
    ("Digit0", 39),
    ("Enter", 40),
    ("Escape", 41),
    ("Backspace", 42),
    ("Tab", 43),
    ("Space", 44),
    ("Minus", 45),
    ("Equal", 46),
    ("BracketLeft", 47),
    ("BracketRight", 48),
    ("Backslash", 49),
    ("Semicolon", 51),
    ("Quote", 52),
    ("Backquote", 53),
    ("Comma", 54),
    ("Period", 55),
    ("Slash", 56),
    ("CapsLock", 57),
    ("F1", 58),
    ("F2", 59),
    ("F3", 60),
    ("F4", 61),
    ("F5", 62),
    ("F6", 63),
    ("F7", 64),
    ("F8", 65),
    ("F9", 66),
    ("F10", 67),
    ("F11", 68),
    ("F12", 69),
    ("PrintScreen", 70),
    ("ScrollLock", 71),
    ("Pause", 72),
    ("Insert", 73),
    ("Home", 74),
    ("PageUp", 75),
    ("Delete", 76),
    ("End", 77),
    ("PageDown", 78),
    ("ArrowRight", 79),
    ("ArrowLeft", 80),
    ("ArrowDown", 81),
    ("ArrowUp", 82),
    ("NumLock", 83),
    ("NumpadDivide", 84),
    ("NumpadMultiply", 85),
    ("NumpadSubtract", 86),
    ("NumpadAdd", 87),
    ("NumpadDecimal", 99),
    ("ContextMenu", 101),
    ("ControlLeft", 224),
    ("ShiftLeft", 225),
    ("AltLeft", 226),
    ("MetaLeft", 227),
    ("MetaRight", 231),
];

#[test]
fn key_table_matches_mxw() {
    for (name, usage) in MXW_KEYS {
        let key = Key::from_name(name).unwrap_or_else(|err| panic!("{name}: {err}"));
        assert_eq!(key.usage(), usage, "{name}");
        assert_eq!(key.name(), name, "{name}");
    }
}

#[test]
fn modifier_keys_carry_their_bit() {
    // the bits mxw's `RAW_KEYS` gives each modifier key
    let cases = [
        ("ControlLeft", 0x01),
        ("ShiftLeft", 0x02),
        ("AltLeft", 0x04),
        ("MetaLeft", 0x08),
        ("MetaRight", 0x80),
    ];
    for (name, bit) in cases {
        assert_eq!(
            Key::from_name(name)
                .unwrap()
                .modifier()
                .map(Modifiers::bits),
            Some(bit),
            "{name}"
        );
    }
    assert_eq!(Key::from_name("KeyA").unwrap().modifier(), None);
}

#[test]
fn bind_retries_on_resend_request() {
    let fake = Fake::default();
    fake.reply((3, 0x00), rep(&[(0, 0xA2)]));
    device(&fake)
        .bind(p(1), Button::Back, Binding::None)
        .unwrap();
    let sent = fake.sent();
    assert_eq!(sent.len(), 2);
    assert_eq!(sent[0], sent[1]);
}

#[test]
fn bind_fails_when_never_acknowledged() {
    let fake = Fake::default();
    for _ in 0..3 {
        fake.reply((3, 0x00), rep(&[(0, 0xA0)]));
    }
    let err = device(&fake)
        .bind(p(1), Button::Back, Binding::None)
        .unwrap_err();
    assert!(
        matches!(err, Error::BindNotAcknowledged { attempts: 3 }),
        "{err:?}"
    );
    assert!(
        fake.replies.borrow()[&(3, 0x00)].is_empty(),
        "all three acknowledgement polls were read"
    );
}

#[test]
fn bind_keeps_waiting_through_two_busy_replies() {
    let fake = Fake::default();
    fake.reply((3, 0x00), rep(&[(0, 0xA0)]));
    fake.reply((3, 0x00), rep(&[(0, 0xA0)]));
    fake.reply((3, 0x00), rep(&[(0, 0x01)]));
    device(&fake)
        .bind(p(1), Button::Back, Binding::None)
        .unwrap();
    assert!(
        fake.replies.borrow()[&(3, 0x00)].is_empty(),
        "the third poll read the acknowledgement"
    );
}

#[test]
fn bind_stops_reading_once_the_mouse_reports_asleep() {
    let fake = Fake::default();
    fake.reply((3, 0x00), rep(&[(0, 0xA4)]));
    fake.reply((3, 0x00), rep(&[(0, 0x01)]));
    let err = device(&fake)
        .bind(p(1), Button::Back, Binding::None)
        .unwrap_err();
    assert!(
        matches!(err, Error::BindNotAcknowledged { attempts: 3 }),
        "{err:?}"
    );
    assert_eq!(
        fake.replies.borrow()[&(3, 0x00)].len(),
        1,
        "the acknowledgement after 0xA4 stays unread"
    );
    assert_eq!(fake.sent().len(), 1, "0xA4 never triggers a resend");
}

#[test]
fn set_scroll_invert_rebinds_every_profile() {
    let fake = Fake::default();
    device(&fake).set_scroll(ScrollDirection::Invert).unwrap();
    let mut expected = Vec::new();
    for profile in 1..=3 {
        for (button, target) in [(16, 17), (17, 16)] {
            expected.push(req(&[
                (4, 0x09),
                (5, 0x03),
                (7, profile),
                (8, button),
                (10, 1),
                (11, 1),
                (12, target),
            ]));
        }
    }
    assert_eq!(fake.sent(), expected);
}

// ---------------------------------------------------------------- write count

/// The write reports `fake` was handed, failed sends included: every read
/// request carries a `0x8N` command, every write a `0x0N` one.
fn writes_handed(fake: &Fake) -> u64 {
    fake.sent().iter().filter(|r| r[6] < 0x80).count() as u64
}

#[test]
fn writes_sent_counts_each_write_report_a_setter_hands_over() {
    let fake = Fake::default();
    let device = device(&fake);
    assert_eq!(device.writes_sent(), 0, "a fresh device");

    device.set_profile(p(2)).unwrap();
    assert_eq!(device.writes_sent(), 1, "set_profile: one report");

    let stages = DpiStages::new(vec![Dpi::new(400).unwrap(), Dpi::new(800).unwrap()]).unwrap();
    device.set_dpi_stages(p(1), &stages).unwrap();
    assert_eq!(
        device.writes_sent(),
        3,
        "set_dpi_stages: stage 1, then the list"
    );

    stage_count_reply(&fake, 3);
    device
        .set_dpi_stage(p(1), DpiStage::new(3).unwrap())
        .unwrap();
    assert_eq!(
        device.writes_sent(),
        4,
        "set_dpi_stage: the count read adds none"
    );

    device
        .set_brightness(Brightness {
            wired: 200,
            wireless: 50,
        })
        .unwrap();
    assert_eq!(
        device.writes_sent(),
        6,
        "set_brightness: wired, then wireless"
    );

    device.set_scroll(ScrollDirection::Invert).unwrap();
    assert_eq!(
        device.writes_sent(),
        12,
        "set_scroll: two binds per profile"
    );

    fake.reply((3, 0x00), rep(&[(0, 0xA2)]));
    device.bind(p(1), Button::Back, Binding::None).unwrap();
    assert_eq!(
        device.writes_sent(),
        14,
        "bind: the report, then its resend"
    );

    assert_eq!(device.writes_sent(), writes_handed(&fake));
}

#[test]
fn reads_and_refusals_before_a_report_count_no_write() {
    let fake = Fake::default();
    let device = device(&fake);
    // Every read gives up on the fake's stale replies; each still sends its request.
    let _ = device.status();
    let _ = device.battery();
    let _ = device.firmware();
    let _ = device.profile();
    let _ = device.dpi(p(1));
    let _ = device.polling_rate();
    let _ = device.lift_off();
    let _ = device.debounce(p(1));
    let _ = device.sleep_delay();
    assert!(!fake.sent().is_empty(), "premise: the reads sent requests");
    assert_eq!(device.writes_sent(), 0, "reads");

    stage_count_reply(&fake, 2);
    device
        .set_dpi_stage(p(1), DpiStage::new(3).unwrap())
        .unwrap_err();
    assert_eq!(device.writes_sent(), 0, "a stage past the count");

    stage_count_reply(&fake, 2);
    device
        .set_dpi_colors(p(1), &[rgb("ff0000"), rgb("00ff00"), rgb("0000ff")])
        .unwrap_err();
    assert_eq!(
        device.writes_sent(),
        0,
        "a colour list longer than the count"
    );

    // no count reply queued: the count read itself fails
    device
        .set_dpi_stage(p(1), DpiStage::new(1).unwrap())
        .unwrap_err();
    assert_eq!(device.writes_sent(), 0, "a failed count read");

    assert_eq!(writes_handed(&fake), 0, "premise: no write report was sent");
}

#[test]
fn a_write_report_whose_send_fails_still_counts() {
    let fake = Fake::default();
    fake.fail_writes.set(true);
    let device = device(&fake);

    let err = device.set_polling_rate(PollingRate::Hz500).unwrap_err();
    assert!(matches!(err, Error::Transport(_)), "{err:?}");
    assert_eq!(device.writes_sent(), 1, "the failed report may have landed");

    device
        .set_brightness(Brightness {
            wired: 200,
            wireless: 50,
        })
        .unwrap_err();
    assert_eq!(
        device.writes_sent(),
        2,
        "the wired report failed, the wireless one was never handed over"
    );
    assert_eq!(device.writes_sent(), writes_handed(&fake));
}

// ---------------------------------------------------------------- values

/// Asserts `result` is `OutOfRange` with exactly these fields.
#[track_caller]
fn assert_out_of_range<T: std::fmt::Debug>(result: gctl_proto::Result<T>, bounds: (i64, i64, i64)) {
    let (value, min, max) = bounds;
    match result {
        Err(Error::OutOfRange {
            value: v,
            min: lo,
            max: hi,
            ..
        }) => assert_eq!((v, lo, hi), (value, min, max)),
        other => panic!("expected OutOfRange {bounds:?}, got {other:?}"),
    }
}

#[test]
fn value_ranges() {
    assert_out_of_range(Profile::new(0), (0, 1, 3));
    assert_out_of_range(Profile::new(4), (4, 1, 3));
    assert_eq!(Profile::new(3).unwrap().get(), 3);

    assert_out_of_range(Dpi::new(99), (99, 100, 19000));
    assert_out_of_range(Dpi::new(19001), (19001, 100, 19000));
    assert_eq!(Dpi::new(100).unwrap().get(), 100);
    assert_eq!(Dpi::new(19000).unwrap().get(), 19000);

    assert_out_of_range(DpiStages::new(vec![]), (0, 1, 6));
    assert_out_of_range(DpiStages::new(vec![Dpi::new(400).unwrap(); 7]), (7, 1, 6));
    assert!(DpiStages::new(vec![Dpi::new(400).unwrap(); 6]).is_ok());

    assert_out_of_range(DpiStage::new(0), (0, 1, 6));
    assert_out_of_range(DpiStage::new(7), (7, 1, 6));

    assert_out_of_range(Debounce::new(17), (17, 0, 16));
    assert_eq!(Debounce::new(0).unwrap().get(), 0);

    assert_out_of_range(Rate::new(101), (101, 0, 100));
    assert_eq!(Rate::DEFAULT.get(), 40);

    assert_eq!(PollingRate::from_ms(4).unwrap(), PollingRate::Hz250);
    assert!(matches!(
        PollingRate::from_ms(3),
        Err(Error::Implausible { value: 3, .. })
    ));
}

#[test]
fn rgb_parses_six_hex_digits() {
    assert_eq!(
        rgb("ff8800"),
        Rgb {
            r: 255,
            g: 136,
            b: 0
        }
    );
    assert_eq!(rgb("FF8800").to_string(), "ff8800");
    assert!("fff".parse::<Rgb>().is_err());
    assert!("gg0000".parse::<Rgb>().is_err());
    assert!("+f8800".parse::<Rgb>().is_err());
}

#[test]
fn rgb_error_names_the_bad_input() {
    assert!(matches!(
        "zzzzzz".parse::<Rgb>(),
        Err(Error::InvalidColour(s)) if s == "zzzzzz"
    ));
    assert!(matches!(
        "fff".parse::<Rgb>(),
        Err(Error::InvalidColour(s)) if s == "fff"
    ));
    assert!(matches!(
        "+f8800".parse::<Rgb>(),
        Err(Error::InvalidColour(s)) if s == "+f8800"
    ));
    // six bytes, three characters: a byte-length check alone lets it through.
    assert!(matches!(
        "ффф".parse::<Rgb>(),
        Err(Error::InvalidColour(s)) if s == "ффф"
    ));
    assert_eq!(
        "zzzzzz".parse::<Rgb>().unwrap_err().to_string(),
        r#"invalid colour "zzzzzz": expected six hex digits, like ff8800"#
    );
}

// ---------------------------------------------------------------- models

#[test]
fn models_cover_every_known_pid() {
    let pids: Vec<u16> = MODELS.iter().map(|m| m.product_id).collect();
    let mut sorted = pids.clone();
    sorted.sort_unstable();
    assert_eq!(
        sorted,
        vec![
            0x2011, 0x2012, 0x2013, 0x2015, 0x2018, 0x2022, 0x2023, 0x2024, 0x2025, 0x2027, 0x2031,
            0x2034
        ]
    );
    let wired: Vec<u16> = MODELS
        .iter()
        .filter(|m| m.link == Link::Wired)
        .map(|m| m.product_id)
        .collect();
    let mut wired_sorted = wired.clone();
    wired_sorted.sort_unstable();
    assert_eq!(wired_sorted, vec![0x2011, 0x2012, 0x2015, 0x2024, 0x2031]);
}

#[test]
fn only_the_model_o_wireless_is_tested() {
    let wireless = Model::from_pid(0x2022).unwrap();
    assert_eq!(wireless.name, "Model O Wireless");
    assert!(wireless.tested);
    let cable = Model::from_pid(0x2011).unwrap();
    assert_eq!(cable.name, "Model O Wireless");
    assert!(cable.tested);
    let tested: Vec<u16> = MODELS
        .iter()
        .filter(|m| m.tested)
        .map(|m| m.product_id)
        .collect();
    assert_eq!(tested.len(), 2, "{tested:x?}");
    assert!(Model::from_pid(0x9999).is_none());
}

fn cand(product_id: u16, interface: i32) -> Candidate {
    Candidate {
        vendor_id: 0x258A,
        product_id,
        interface,
    }
}

#[test]
fn select_prefers_the_wired_config_interface() {
    let picked = select(&[cand(0x2022, 2), cand(0x2011, 2), cand(0x2011, 0)]).unwrap();
    assert_eq!(picked, cand(0x2011, 2));

    // O- wired has the higher pid; a lowest-pid pick would choose the wireless one.
    let picked = select(&[cand(0x2013, 2), cand(0x2024, 2)]).unwrap();
    assert_eq!(picked, cand(0x2024, 2));
}

#[test]
fn select_ignores_foreign_and_non_config_interfaces() {
    assert!(select(&[cand(0x2022, 0), cand(0x2022, 1)]).is_none());
    assert!(select(&[cand(0x9999, 2)]).is_none());
    let foreign = Candidate {
        vendor_id: 0x046D,
        product_id: 0x2022,
        interface: 2,
    };
    assert!(select(&[foreign]).is_none());
}

#[test]
fn select_falls_back_to_wireless_without_a_wired_config_interface() {
    let picked = select(&[cand(0x2011, 0), cand(0x2022, 2)]).unwrap();
    assert_eq!(picked, cand(0x2022, 2));
}

#[cfg(feature = "hidapi")]
#[test]
fn open_takes_a_hid_api_and_may_find_no_mouse() {
    // signature pin only: calling it would enumerate the real hidraw devices.
    let _: fn(&hidapi::HidApi) -> gctl_proto::Result<Option<Device<hidapi::HidDevice>>> =
        Device::<hidapi::HidDevice>::open;
}
