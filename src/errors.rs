use std::fmt;
use std::sync::Arc;

use crate::blocks::{block_name, BlockType};

pub use std::error::Error as StdError;
pub use std::result::Result as StdResult;

/// Result type returned from functions that can have our `Error`s.
pub type Result<T> = StdResult<T, Error>;

/// Swaystatus' error type
#[derive(Debug, Clone)]
pub struct Error {
    pub kind: ErrorKind,
    pub message: Option<String>,
    pub cause: Option<Arc<dyn StdError + Send + Sync + 'static>>,
    pub block: Option<(BlockType, usize)>,
}

/// A set of errors that can occur during the runtime of swaystatus
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ErrorKind {
    Config,
    Format,
    Other,
}

impl Error {
    pub fn new<T: Into<String>>(message: T) -> Self {
        Self {
            kind: ErrorKind::Other,
            message: Some(message.into()),
            cause: None,
            block: None,
        }
    }

    pub fn new_format<T: Into<String>>(message: T) -> Self {
        Self {
            kind: ErrorKind::Format,
            message: Some(message.into()),
            cause: None,
            block: None,
        }
    }
}

pub trait InBlock {
    fn in_block(self, block: BlockType, block_id: usize) -> Self;
}

impl<T> InBlock for Result<T> {
    fn in_block(self, block: BlockType, block_id: usize) -> Self {
        self.map_err(|mut e| {
            e.block = Some((block, block_id));
            e
        })
    }
}

pub trait ResultExt<T> {
    fn error<M: Into<String>>(self, message: M) -> Result<T>;
    fn or_error<M: Into<String>, F: FnOnce() -> M>(self, f: F) -> Result<T>;
    fn config_error(self) -> Result<T>;
    fn format_error<M: Into<String>>(self, message: M) -> Result<T>;
}

impl<T, E: StdError + Send + Sync + 'static> ResultExt<T> for StdResult<T, E> {
    fn error<M: Into<String>>(self, message: M) -> Result<T> {
        self.map_err(|e| Error {
            kind: ErrorKind::Other,
            message: Some(message.into()),
            cause: Some(Arc::new(e)),
            block: None,
        })
    }

    fn or_error<M: Into<String>, F: FnOnce() -> M>(self, f: F) -> Result<T> {
        self.map_err(|e| Error {
            kind: ErrorKind::Other,
            message: Some(f().into()),
            cause: Some(Arc::new(e)),
            block: None,
        })
    }

    fn config_error(self) -> Result<T> {
        self.map_err(|e| Error {
            kind: ErrorKind::Config,
            message: None,
            cause: Some(Arc::new(e)),
            block: None,
        })
    }

    fn format_error<M: Into<String>>(self, message: M) -> Result<T> {
        self.map_err(|e| Error {
            kind: ErrorKind::Format,
            message: Some(message.into()),
            cause: Some(Arc::new(e)),
            block: None,
        })
    }
}

pub trait OptionExt<T> {
    fn error<M: Into<String>>(self, message: M) -> Result<T>;
    fn or_error<M: Into<String>, F: FnOnce() -> M>(self, f: F) -> Result<T>;
    fn config_error(self) -> Result<T>;
    fn format_error<M: Into<String>>(self, message: M) -> Result<T>;
}

impl<T> OptionExt<T> for Option<T> {
    fn error<M: Into<String>>(self, message: M) -> Result<T> {
        self.ok_or_else(|| Error {
            kind: ErrorKind::Other,
            message: Some(message.into()),
            cause: None,
            block: None,
        })
    }

    fn or_error<M: Into<String>, F: FnOnce() -> M>(self, f: F) -> Result<T> {
        self.ok_or_else(|| Error {
            kind: ErrorKind::Other,
            message: Some(f().into()),
            cause: None,
            block: None,
        })
    }

    fn config_error(self) -> Result<T> {
        self.ok_or(Error {
            kind: ErrorKind::Config,
            message: None,
            cause: None,
            block: None,
        })
    }

    fn format_error<M: Into<String>>(self, message: M) -> Result<T> {
        self.ok_or_else(|| Error {
            kind: ErrorKind::Format,
            message: Some(message.into()),
            cause: None,
            block: None,
        })
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self.block {
            Some(block) => {
                match self.kind {
                    ErrorKind::Config | ErrorKind::Format => f.write_str("Configuration errror")?,
                    ErrorKind::Other => f.write_str("Error")?,
                }

                write!(f, " in {}", block_name(block.0))?;

                if let Some(message) = &self.message {
                    write!(f, ": {}", message)?;
                }

                if let Some(cause) = &self.cause {
                    write!(f, ". (Cause: {})", cause)?;
                }
            }
            None => {
                f.write_str(self.message.as_deref().unwrap_or("Error"))?;
                if let Some(cause) = &self.cause {
                    write!(f, ". (Cause: {})", cause)?;
                }
            }
        }

        Ok(())
    }
}

impl StdError for Error {}

pub trait ToSerdeError<T> {
    fn serde_error<E: serde::de::Error>(self) -> StdResult<T, E>;
}

impl<T, F> ToSerdeError<T> for StdResult<T, F>
where
    F: fmt::Display,
{
    fn serde_error<E: serde::de::Error>(self) -> StdResult<T, E> {
        self.map_err(E::custom)
    }
}
