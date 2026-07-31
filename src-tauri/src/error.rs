use serde::ser::{Serialize, SerializeStruct, Serializer};

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Homebrew was not found on this system. Install it from https://brew.sh, then restart own-brew.")]
    BrewNotFound,

    #[error("`brew {command}` exited with code {code}")]
    BrewFailed {
        command: String,
        code: i32,
        stderr: String,
    },

    #[error("`brew {command}` was terminated before it finished")]
    BrewTerminated { command: String },

    #[error("could not understand the output of `brew {command}`")]
    Parse {
        command: String,
        #[source]
        source: serde_json::Error,
    },

    #[error("the operation was cancelled")]
    Cancelled,

    #[error("no operation is running with id {0}")]
    UnknownOperation(u64),

    #[error("another operation is already running")]
    OperationInFlight,

    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error("network request failed: {0}")]
    Http(#[from] reqwest::Error),

    #[error("local database error: {0}")]
    Db(#[from] rusqlite::Error),

    #[error("{0}")]
    Catalog(String),
}

impl Error {
    /// Stable machine-readable discriminant. The UI branches on this, never on
    /// the human-readable message.
    pub fn kind(&self) -> &'static str {
        match self {
            Error::BrewNotFound => "brew_not_found",
            Error::BrewFailed { .. } => "brew_failed",
            Error::BrewTerminated { .. } => "brew_terminated",
            Error::Parse { .. } => "parse",
            Error::Cancelled => "cancelled",
            Error::UnknownOperation(_) => "unknown_operation",
            Error::OperationInFlight => "operation_in_flight",
            Error::Io(_) => "io",
            Error::Http(_) => "http",
            Error::Db(_) => "db",
            Error::Catalog(_) => "catalog",
        }
    }

    /// Extra context worth showing verbatim, such as Homebrew's own stderr.
    pub fn detail(&self) -> Option<&str> {
        match self {
            Error::BrewFailed { stderr, .. } if !stderr.is_empty() => Some(stderr),
            _ => None,
        }
    }
}

impl Serialize for Error {
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        let mut s = serializer.serialize_struct("Error", 3)?;
        s.serialize_field("kind", self.kind())?;
        s.serialize_field("message", &self.to_string())?;
        s.serialize_field("detail", &self.detail())?;
        s.end()
    }
}
