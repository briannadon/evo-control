//! Background worker thread: owns the Driver, serializes all device I/O.
//!
//! Two spawn modes:
//!   * `spawn()` — request/reply only. Used by the CLI for one-shot ops.
//!   * `spawn_polling(interval)` — additionally polls `driver.status()` on a
//!     ticker and pushes snapshots to a bounded(1) "latest" channel. Used by
//!     the GUI so the egui thread never blocks on USB I/O.
//!
//! Each write request carries an `Option<Reply<_>>`: callers that need the
//! result (CLI) pass `Some` and wait; callers that don't (GUI knobs) pass
//! `None` and never block on the worker.

use std::thread;
use std::time::{Duration, Instant};

use crossbeam_channel::{bounded, never, tick, Receiver, Sender};

use crate::{driver::Driver, error::DriverError, state::DeviceStatus};

type Reply<T> = Sender<Result<T, DriverError>>;

pub enum Request {
    Status(Reply<DeviceStatus>),
    VolumeSet { pair: u8, db: f32, reply: Option<Reply<f32>> },
    GainSet { input: u8, db: f32, reply: Option<Reply<f32>> },
    PhantomSet { input: u8, on: bool, reply: Option<Reply<()>> },
    InputMuteSet { input: u8, muted: bool, reply: Option<Reply<()>> },
    OutputMuteSet { muted: bool, reply: Option<Reply<()>> },
    MixerSet { in_idx: u8, out_idx: u8, db: f32, reply: Option<Reply<()>> },
    Shutdown,
}

/// Cheap-clone handle for sending requests to the worker thread.
#[derive(Clone)]
pub struct DriverHandle {
    tx: Sender<Request>,
}

impl DriverHandle {
    // ── Blocking calls (CLI): wait for reply ──────────────────────────────

    pub fn status(&self) -> Result<DeviceStatus, DriverError> {
        self.call(|reply| Request::Status(reply))
    }

    pub fn volume_set(&self, pair: u8, db: f32) -> Result<f32, DriverError> {
        self.call(|reply| Request::VolumeSet { pair, db, reply: Some(reply) })
    }

    pub fn gain_set(&self, input: u8, db: f32) -> Result<f32, DriverError> {
        self.call(|reply| Request::GainSet { input, db, reply: Some(reply) })
    }

    pub fn phantom_set(&self, input: u8, on: bool) -> Result<(), DriverError> {
        self.call(|reply| Request::PhantomSet { input, on, reply: Some(reply) })
    }

    pub fn input_mute_set(&self, input: u8, muted: bool) -> Result<(), DriverError> {
        self.call(|reply| Request::InputMuteSet { input, muted, reply: Some(reply) })
    }

    pub fn output_mute_set(&self, muted: bool) -> Result<(), DriverError> {
        self.call(|reply| Request::OutputMuteSet { muted, reply: Some(reply) })
    }

    pub fn mixer_set(&self, in_idx: u8, out_idx: u8, db: f32) -> Result<(), DriverError> {
        self.call(|reply| Request::MixerSet { in_idx, out_idx, db, reply: Some(reply) })
    }

    // ── Fire-and-forget (GUI): never block on the worker ──────────────────

    pub fn volume_set_async(&self, pair: u8, db: f32) {
        let _ = self.tx.try_send(Request::VolumeSet { pair, db, reply: None });
    }

    pub fn gain_set_async(&self, input: u8, db: f32) {
        let _ = self.tx.try_send(Request::GainSet { input, db, reply: None });
    }

    pub fn phantom_set_async(&self, input: u8, on: bool) {
        let _ = self.tx.try_send(Request::PhantomSet { input, on, reply: None });
    }

    pub fn input_mute_set_async(&self, input: u8, muted: bool) {
        let _ = self.tx.try_send(Request::InputMuteSet { input, muted, reply: None });
    }

    pub fn output_mute_set_async(&self, muted: bool) {
        let _ = self.tx.try_send(Request::OutputMuteSet { muted, reply: None });
    }

    pub fn mixer_set_async(&self, in_idx: u8, out_idx: u8, db: f32) {
        let _ = self.tx.try_send(Request::MixerSet { in_idx, out_idx, db, reply: None });
    }

    pub fn shutdown(&self) {
        let _ = self.tx.send(Request::Shutdown);
    }

    fn call<T: Send + 'static>(&self, mk: impl FnOnce(Reply<T>) -> Request) -> Result<T, DriverError> {
        let (tx, rx) = bounded(1);
        self.tx.send(mk(tx)).map_err(|_| DriverError::Disconnected)?;
        rx.recv().map_err(|_| DriverError::Disconnected)?
    }
}

/// Spawn a request/reply-only worker. `disc_rx` fires once when the worker
/// exits (device gone or `Shutdown`).
pub fn spawn() -> Result<(DriverHandle, Receiver<()>), DriverError> {
    let driver = Driver::open()?;
    let (tx, rx) = bounded::<Request>(64);
    let (disc_tx, disc_rx) = bounded::<()>(1);

    thread::Builder::new()
        .name("evo-driver".into())
        .spawn(move || {
            run_loop(driver, rx, None, None);
            let _ = disc_tx.send(());
        })
        .expect("failed to spawn driver thread");

    Ok((DriverHandle { tx }, disc_rx))
}

/// Spawn a worker that also polls device status on `interval` and pushes
/// snapshots to a bounded(1) "latest" channel. The GUI uses this so its main
/// thread never calls into USB I/O directly.
pub fn spawn_polling(
    interval: Duration,
) -> Result<(DriverHandle, Receiver<()>, Receiver<DeviceStatus>), DriverError> {
    let driver = Driver::open()?;
    let (tx, rx) = bounded::<Request>(64);
    let (disc_tx, disc_rx) = bounded::<()>(1);
    let (stat_tx, stat_rx) = bounded::<DeviceStatus>(1);

    thread::Builder::new()
        .name("evo-driver".into())
        .spawn(move || {
            run_loop(driver, rx, Some(stat_tx), Some(interval));
            let _ = disc_tx.send(());
        })
        .expect("failed to spawn driver thread");

    Ok((DriverHandle { tx }, disc_rx, stat_rx))
}

fn run_loop(
    driver: Driver,
    rx: Receiver<Request>,
    status_tx: Option<Sender<DeviceStatus>>,
    interval: Option<Duration>,
) {
    let ticker: Receiver<Instant> = match interval {
        Some(d) => tick(d),
        None => never(),
    };

    loop {
        crossbeam_channel::select! {
            recv(rx) -> msg => {
                let msg = match msg {
                    Ok(m) => m,
                    Err(_) => break, // all handles dropped
                };
                if !handle_request(&driver, msg) {
                    break;
                }
            }
            recv(ticker) -> _ => {
                let Some(tx) = status_tx.as_ref() else { continue };
                match driver.status() {
                    Ok(s) => { let _ = tx.try_send(s); }
                    // Any error during a poll: exit so the GUI's disc_rx
                    // fires and it spawns a fresh worker on a fresh fd.
                    // The watchdog would catch staleness anyway, but
                    // breaking immediately recovers faster.
                    Err(_) => break,
                }
            }
        }
    }
}

/// Returns `false` to signal the loop should exit (Shutdown).
fn handle_request(driver: &Driver, msg: Request) -> bool {
    match msg {
        Request::Status(reply) => { let _ = reply.send(driver.status()); }
        Request::VolumeSet { pair, db, reply } => {
            let r = driver.volume_set(pair, db);
            if let Some(rep) = reply { let _ = rep.send(r); }
        }
        Request::GainSet { input, db, reply } => {
            let r = driver.gain_set(input, db);
            if let Some(rep) = reply { let _ = rep.send(r); }
        }
        Request::PhantomSet { input, on, reply } => {
            let r = driver.phantom_set(input, on);
            if let Some(rep) = reply { let _ = rep.send(r); }
        }
        Request::InputMuteSet { input, muted, reply } => {
            let r = driver.input_mute_set(input, muted);
            if let Some(rep) = reply { let _ = rep.send(r); }
        }
        Request::OutputMuteSet { muted, reply } => {
            let r = driver.output_mute_set(muted);
            if let Some(rep) = reply { let _ = rep.send(r); }
        }
        Request::MixerSet { in_idx, out_idx, db, reply } => {
            let r = driver.mixer_set(in_idx, out_idx, db);
            if let Some(rep) = reply { let _ = rep.send(r); }
        }
        Request::Shutdown => return false,
    }
    true
}
