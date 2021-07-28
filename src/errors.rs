use std::error::Error as StdError;
use std::fmt;

pub use std::result::Result as StdResult;
pub use Error::*;

/// Result type returned from functions that can have our `Error`s.
pub type Result<T> = StdResult<T, Error>;

/// A set of errors that can occur during the runtime of swaystatus
#[derive(Clone, Debug)]
pub enum Error {
    /// An error that occurred in the block
    BlockError {
        block: String,
        message: String,
        cause: Option<String>,
        cause_dbg: Option<String>,
    },
    /// An error that occurred because of mistake in the config file
    ConfigError {
        block: Option<String>,
        cause: String,
        cause_dbg: String,
    },
    /// An error that occurred outside of any block
    InternalError {
        context: String,
        message: String,
        cause: Option<String>,
        cause_dbg: Option<String>,
    },
    /// Just a message, no additional info. Use it when you don't care about what caused the error.
    /// Also use it when it's know that this error will be propagated and converted to one of more
    /// specific error types.
    Message { message: String },
}

pub trait ResultExt<T, E> {
    fn block_error(self, block: &str, message: &str) -> Result<T>;
    fn config_error(self) -> Result<T>;
    fn block_config_error(self, block: &str) -> Result<T>;
    fn internal_error(self, context: &str, message: &str) -> Result<T>;
    fn with_message(self, message: &str) -> Result<T>;
}

impl<T, E> ResultExt<T, E> for StdResult<T, E>
where
    E: StdError,
{
    fn block_error(self, block: &str, message: &str) -> Result<T> {
        self.map_err(|e| BlockError {
            block: block.to_owned(),
            message: message.to_owned(),
            cause: Some(e.to_string()),
            cause_dbg: Some(format!("{:?}", e)),
        })
    }

    fn config_error(self) -> Result<T> {
        self.map_err(|e| ConfigError {
            block: None,
            cause: e.to_string(),
            cause_dbg: format!("{:?}", e),
        })
    }

    fn block_config_error(self, block: &str) -> Result<T> {
        self.map_err(|e| ConfigError {
            block: Some(block.to_string()),
            cause: e.to_string(),
            cause_dbg: format!("{:?}", e),
        })
    }

    fn internal_error(self, context: &str, message: &str) -> Result<T> {
        self.map_err(|e| InternalError {
            context: context.to_string(),
            message: message.to_string(),
            cause: Some(e.to_string()),
            cause_dbg: Some(format!("{:?}", e)),
        })
    }

    fn with_message(self, message: &str) -> Result<T> {
        self.map_err(|_| Message {
            message: message.to_string(),
        })
    }
}

pub trait OptionExt<T> {
    fn block_error(self, block: &str, message: &str) -> Result<T>;
    fn config_error(self, message: &str) -> Result<T>;
    fn internal_error(self, context: &str, message: &str) -> Result<T>;
    fn with_message(self, message: &str) -> Result<T>;
}

impl<T> OptionExt<T> for Option<T> {
    fn block_error(self, block: &str, message: &str) -> Result<T> {
        self.ok_or_else(|| BlockError {
            block: block.to_owned(),
            message: message.to_owned(),
            cause: None,
            cause_dbg: None,
        })
    }

    fn config_error(self, message: &str) -> Result<T> {
        self.ok_or_else(|| ConfigError {
            block: None,
            cause: message.to_string(),
            cause_dbg: message.to_string(),
        })
    }

    fn internal_error(self, context: &str, message: &str) -> Result<T> {
        self.ok_or_else(|| InternalError {
            context: context.to_owned(),
            message: message.to_owned(),
            cause: None,
            cause_dbg: None,
        })
    }

    fn with_message(self, message: &str) -> Result<T> {
        self.ok_or_else(|| Message {
            message: message.to_string(),
        })
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            BlockError {
                block,
                message,
                cause,
                ..
            } => {
                if let Some(cause) = cause {
                    write!(
                        f,
                        "Error in block '{}': {}. Cause: {}",
                        block, message, cause
                    )
                } else {
                    write!(f, "Error in block '{}': {}", block, message)
                }
            }
            ConfigError { block, cause, .. } => {
                if let Some(block) = block {
                    write!(
                        f,
                        "Configuration error in block '{}'. Cause: {}",
                        block, cause
                    )
                } else {
                    write!(f, "Configuration error. Cause: {}", cause)
                }
            }
            InternalError {
                context,
                message,
                cause,
                ..
            } => {
                if let Some(cause) = cause {
                    write!(
                        f,
                        "Internal error in '{}': {}. Cause: {}",
                        context, message, cause
                    )
                } else {
                    write!(f, "Internal error in '{}': {}", context, message)
                }
            }
            Message { message } => {
                write!(f, "{}", message)
            }
        }
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
        self.map_err(|e| E::custom(e.to_string()))
    }
}
