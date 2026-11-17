use thiserror::Error;

#[derive(Error, Debug)]
pub enum DroboError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("SCSI IOCTL failed with status: {0:#x}")]
    IoctlStatusError(u8),

    #[error("SCSI IOCTL raw error")]
    IoctlError,

    #[error("Device query output length unexpected (expected {expected}, got {got})")]
    UnexpectedLength { expected: usize, got: usize },

    #[error("Device is not a valid Drobo unit: {0}")]
    InvalidDevice(String),

    #[error("Firmware validation failed: {0}")]
    InvalidFirmware(String),

    #[error("Network error: {0}")]
    Network(String),

    #[error("Command failed: {0}")]
    CommandFailed(String),
}

pub type Result<T> = std::result::Result<T, DroboError>;
