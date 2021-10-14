use std::fmt;
use std::sync::Arc;

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
    pub block: Option<&'static str>,
}

/// A set of errors that can occur during the runtime of swaystatus
#[derive(Debug, Clone)]
pub enum ErrorKind {
    Config,
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
}

// impl<E: StdError> From<E> for Error {
//     fn from(err: E) -> Self {
//         Self {
//             kind: ErrorKind::Other,
//             message: None,
//             cause: Arc::new(err),
//         }
//     }
// }

pub trait ResultExt<T> {
    fn error<M: Into<String>>(self, message: M) -> Result<T>;
    fn config_error(self) -> Result<T>;
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

    fn config_error(self) -> Result<T> {
        self.map_err(|e| Error {
            kind: ErrorKind::Config,
            message: None,
            cause: Some(Arc::new(e)),
            block: None,
        })
    }
}

pub trait OptionExt<T> {
    fn error<M: Into<String>>(self, message: M) -> Result<T>;
    fn config_error(self) -> Result<T>;
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

    fn config_error(self) -> Result<T> {
        self.ok_or(Error {
            kind: ErrorKind::Config,
            message: None,
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
                    ErrorKind::Config => f.write_str("Configuration errror")?,
                    ErrorKind::Other => f.write_str("Error")?,
                }

                write!(f, " in {}", block)?;

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
