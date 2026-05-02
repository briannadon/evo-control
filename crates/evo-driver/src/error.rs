use thiserror::Error;

#[derive(Debug, Error)]
pub enum DriverError {
    #[error("kernel module not loaded — run `sudo evo-control install-driver`")]
    KmodNotLoaded,

    #[error("device disconnected")]
    Disconnected,

    #[error("USB control transfer failed: {0}")]
    Transfer(i32),

    #[error("channel index {idx} out of range (max {max})")]
    ChannelIndex { idx: u8, max: u8 },

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}
