//! Background worker thread: owns the Driver, serializes all device I/O.
//!
//! The GUI and CLI use DriverHandle (cheap-clone) to send typed requests.
//! Each request carries a one-shot reply channel so callers can block or
//! select as appropriate.

use std::thread;

use crossbeam_channel::{bounded, Receiver, Sender};

use crate::{driver::Driver, error::DriverError, state::DeviceStatus};

type Reply<T> = Sender<Result<T, DriverError>>;

pub enum Request {
    Status(Reply<DeviceStatus>),
    VolumeSet { pair: u8, db: f32, reply: Reply<f32> },
    GainSet { input: u8, db: f32, reply: Reply<f32> },
    PhantomSet { input: u8, on: bool, reply: Reply<()> },
    InputMuteSet { input: u8, muted: bool, reply: Reply<()> },
    OutputMuteSet { muted: bool, reply: Reply<()> },
    MixerSet { in_idx: u8, out_idx: u8, db: f32, reply: Reply<()> },
    Shutdown,
}

/// Cheap-clone handle for sending requests to the worker thread.
#[derive(Clone)]
pub struct DriverHandle {
    tx: Sender<Request>,
}

impl DriverHandle {
    pub fn status(&self) -> Result<DeviceStatus, DriverError> {
        self.call(|reply| Request::Status(reply))
    }

    pub fn volume_set(&self, pair: u8, db: f32) -> Result<f32, DriverError> {
        self.call(|reply| Request::VolumeSet { pair, db, reply })
    }

    pub fn gain_set(&self, input: u8, db: f32) -> Result<f32, DriverError> {
        self.call(|reply| Request::GainSet { input, db, reply })
    }

    pub fn phantom_set(&self, input: u8, on: bool) -> Result<(), DriverError> {
        self.call(|reply| Request::PhantomSet { input, on, reply })
    }

    pub fn input_mute_set(&self, input: u8, muted: bool) -> Result<(), DriverError> {
        self.call(|reply| Request::InputMuteSet { input, muted, reply })
    }

    pub fn output_mute_set(&self, muted: bool) -> Result<(), DriverError> {
        self.call(|reply| Request::OutputMuteSet { muted, reply })
    }

    pub fn mixer_set(&self, in_idx: u8, out_idx: u8, db: f32) -> Result<(), DriverError> {
        self.call(|reply| Request::MixerSet { in_idx, out_idx, db, reply })
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

/// Spawn the worker thread. Returns `(handle, disconnected_rx)`.
///
/// `disconnected_rx` fires once when the worker exits (device gone or Shutdown).
pub fn spawn() -> Result<(DriverHandle, Receiver<()>), DriverError> {
    let driver = Driver::open()?;
    let (tx, rx) = bounded::<Request>(64);
    let (disc_tx, disc_rx) = bounded::<()>(1);

    thread::Builder::new()
        .name("evo-driver".into())
        .spawn(move || {
            run_loop(driver, rx);
            let _ = disc_tx.send(());
        })
        .expect("failed to spawn driver thread");

    Ok((DriverHandle { tx }, disc_rx))
}

fn run_loop(driver: Driver, rx: Receiver<Request>) {
    for msg in rx {
        match msg {
            Request::Status(reply) => { let _ = reply.send(driver.status()); }
            Request::VolumeSet { pair, db, reply } => { let _ = reply.send(driver.volume_set(pair, db)); }
            Request::GainSet { input, db, reply } => { let _ = reply.send(driver.gain_set(input, db)); }
            Request::PhantomSet { input, on, reply } => { let _ = reply.send(driver.phantom_set(input, on)); }
            Request::InputMuteSet { input, muted, reply } => { let _ = reply.send(driver.input_mute_set(input, muted)); }
            Request::OutputMuteSet { muted, reply } => { let _ = reply.send(driver.output_mute_set(muted)); }
            Request::MixerSet { in_idx, out_idx, db, reply } => { let _ = reply.send(driver.mixer_set(in_idx, out_idx, db)); }
            Request::Shutdown => break,
        }
    }
}
