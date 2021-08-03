use std::error::Error as StdError;
use std::fmt;

pub use std::result::Result as StdResult;

/// Result type returned from functions that can have our `Error`s.
pub type Result<T> = StdResult<T, Error>;

/// A set of errors that can occur during the runtime of swaystatus
#[derive(Clone, Debug)]
pub enum Error {
    /// An error that occurred in the block
    Block {
        block: String,
        message: String,
        cause: Option<String>,
        cause_dbg: Option<String>,
    },
    /// An error that occurred because of mistake in the config file
    Config {
        block: Option<String>,
        cause: String,
        cause_dbg: String,
    },
    /// An error that occurred outside of any block
    Internal {
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

pub fn block_error<T>(block: &str, message: &str) -> Result<T> {
    Err(Error::Block {
        block: block.to_string(),
        message: message.to_string(),
        cause: None,
        cause_dbg: None,
    })
}

pub fn internal_error<T>(context: &str, message: &str) -> Result<T> {
    Err(Error::Internal {
        context: context.to_string(),
        message: message.to_string(),
        cause: None,
        cause_dbg: None,
    })
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
        self.map_err(|e| Error::Block {
            block: block.to_owned(),
            message: message.to_owned(),
            cause: Some(e.to_string()),
            cause_dbg: Some(format!("{:?}", e)),
        })
    }

    fn config_error(self) -> Result<T> {
        self.map_err(|e| Error::Config {
            block: None,
            cause: e.to_string(),
            cause_dbg: format!("{:?}", e),
        })
    }

    fn block_config_error(self, block: &str) -> Result<T> {
        self.map_err(|e| Error::Config {
            block: Some(block.to_string()),
            cause: e.to_string(),
            cause_dbg: format!("{:?}", e),
        })
    }

    fn internal_error(self, context: &str, message: &str) -> Result<T> {
        self.map_err(|e| Error::Internal {
            context: context.to_string(),
            message: message.to_string(),
            cause: Some(e.to_string()),
            cause_dbg: Some(format!("{:?}", e)),
        })
    }

    fn with_message(self, message: &str) -> Result<T> {
        self.map_err(|_| Error::Message {
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
        self.ok_or_else(|| Error::Block {
            block: block.to_owned(),
            message: message.to_owned(),
            cause: None,
            cause_dbg: None,
        })
    }

    fn config_error(self, message: &str) -> Result<T> {
        self.ok_or_else(|| Error::Config {
            block: None,
            cause: message.to_string(),
            cause_dbg: message.to_string(),
        })
    }

    fn internal_error(self, context: &str, message: &str) -> Result<T> {
        self.ok_or_else(|| Error::Internal {
            context: context.to_owned(),
            message: message.to_owned(),
            cause: None,
            cause_dbg: None,
        })
    }

    fn with_message(self, message: &str) -> Result<T> {
        self.ok_or_else(|| Error::Message {
            message: message.to_string(),
        })
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Error::Block {
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
            Error::Config { block, cause, .. } => {
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
            Error::Internal {
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
            Error::Message { message } => {
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
        self.map_err(E::custom)
    }
}
