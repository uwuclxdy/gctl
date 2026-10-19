use crate::transport::TransportError;

pub type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("HID transport failed")]
    Transport(#[source] TransportError),

    #[error("no response to read command {command:#04X}")]
    NoResponse { command: u8 },

    #[error("device reported an implausible {what} ({value})")]
    Implausible { what: &'static str, value: u16 },

    #[error("{what} {value} is out of range {min}..={max}")]
    OutOfRange {
        what: &'static str,
        value: i64,
        min: i64,
        max: i64,
    },

    #[error("profile {profile} has {stages} DPI stage(s); got {given} value(s)")]
    StageCountMismatch {
        profile: u8,
        stages: u8,
        given: usize,
    },

    #[error("device did not acknowledge the bind after {attempts} attempts")]
    BindNotAcknowledged { attempts: usize },

    #[error("unknown key name {0:?}")]
    UnknownKey(String),

    #[error("invalid colour {0:?}: expected six hex digits, like ff8800")]
    InvalidColour(String),

    #[error("device is asleep")]
    Asleep,
}
