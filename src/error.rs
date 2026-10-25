//! Error type returned by the decoder.

use core::fmt;

/// Reason why a stream could not be decoded.
///
/// The decoder never panics on malformed input; every violation of the format
/// is reported as one of these values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    /// The input ended before the stream was complete.
    Truncated,
    /// The stream breaks a rule of the format. The text names the rule.
    Corrupt(&'static str),
    /// The stream is well formed as far as it was read but uses a feature this
    /// crate does not implement (another codec, checksums, ...).
    Unsupported(&'static str),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Truncated => f.write_str("input ended unexpectedly"),
            Error::Corrupt(what) => write!(f, "corrupt stream: {what}"),
            Error::Unsupported(what) => write!(f, "unsupported feature: {what}"),
        }
    }
}

impl std::error::Error for Error {}

/// Result alias used throughout the crate.
pub type Result<T> = core::result::Result<T, Error>;

/// Shorthand for building a [`Error::Corrupt`] result.
pub(crate) fn corrupt<T>(what: &'static str) -> Result<T> {
    Err(Error::Corrupt(what))
}
