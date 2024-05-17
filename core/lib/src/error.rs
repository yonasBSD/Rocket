//! Types representing various errors that can occur in a Rocket application.

use std::{io, fmt, process};
use std::error::Error as StdError;
use std::sync::Arc;

use figment::Profile;

use crate::listener::Endpoint;
use crate::trace::Trace;
use crate::{Ignite, Orbit, Phase, Rocket};

/// An error that occurs during launch.
///
/// An `Error` is returned by [`launch()`](Rocket::launch()) when launching an
/// application fails or, more rarely, when the runtime fails after launching.
///
/// # Usage
///
/// An `Error` value should usually be allowed to `drop` without inspection.
/// There are at least two exceptions:
///
///   1. If you are writing a library or high-level application on-top of
///      Rocket, you likely want to inspect the value before it drops to avoid a
///      Rocket-specific `panic!`. This typically means simply printing the
///      value.
///
///   2. You want to display your own error messages.
pub struct Error {
    pub(crate) kind: ErrorKind
}

/// The kind error that occurred.
///
/// In almost every instance, a launch error occurs because of an I/O error;
/// this is represented by the `Io` variant. A launch error may also occur
/// because of ill-defined routes that lead to collisions or because a fairing
/// encountered an error; these are represented by the `Collision` and
/// `FailedFairing` variants, respectively.
#[derive(Debug)]
#[non_exhaustive]
// FIXME: Don't expose this. Expose access methods from `Error` instead.
pub enum ErrorKind {
    /// Binding to the network interface at `.0` failed with error `.1`.
    Bind(Option<Endpoint>, Box<dyn StdError + Send>),
    /// An I/O error occurred during launch.
    Io(io::Error),
    /// A valid [`Config`](crate::Config) could not be extracted from the
    /// configured figment.
    Config(figment::Error),
    /// Route collisions were detected.
    Collisions(crate::router::Collisions),
    /// Launch fairing(s) failed.
    FailedFairings(Vec<crate::fairing::Info>),
    /// Sentinels requested abort.
    SentinelAborts(Vec<crate::sentinel::Sentry>),
    /// The configuration profile is not debug but no secret key is configured.
    InsecureSecretKey(Profile),
    /// Liftoff failed. Contains the Rocket instance that failed to shutdown.
    Liftoff(
        Result<Box<Rocket<Ignite>>, Arc<Rocket<Orbit>>>,
        tokio::task::JoinError,
    ),
    /// Shutdown failed. Contains the Rocket instance that failed to shutdown.
    Shutdown(Arc<Rocket<Orbit>>),
}

/// An error that occurs when a value was unexpectedly empty.
#[derive(Clone, Copy, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Empty;

impl Error {
    #[inline(always)]
    pub(crate) fn new(kind: ErrorKind) -> Error {
        Error { kind }
    }

    // FIXME: Don't expose this. Expose finer access methods instead.
    pub fn kind(&self) -> &ErrorKind {
        &self.kind
    }

    pub fn report<P: Phase>(result: Result<Rocket<P>, Error>) -> process::ExitCode {
        match result {
            Ok(_) => process::ExitCode::SUCCESS,
            Err(e) => {
                error_span!("aborting launch due to error" => e.trace_error());
                process::ExitCode::SUCCESS
            }
        }
    }
}

impl From<ErrorKind> for Error {
    fn from(kind: ErrorKind) -> Self {
        Error::new(kind)
    }
}

impl From<figment::Error> for Error {
    fn from(e: figment::Error) -> Self {
        Error::new(ErrorKind::Config(e))
    }
}

impl From<io::Error> for Error {
    fn from(e: io::Error) -> Self {
        Error::new(ErrorKind::Io(e))
    }
}

impl StdError for Error {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        match &self.kind {
            ErrorKind::Bind(_, e) => Some(&**e),
            ErrorKind::Io(e) => Some(e),
            ErrorKind::Collisions(_) => None,
            ErrorKind::FailedFairings(_) => None,
            ErrorKind::InsecureSecretKey(_) => None,
            ErrorKind::Config(e) => Some(e),
            ErrorKind::SentinelAborts(_) => None,
            ErrorKind::Liftoff(_, e) => Some(e),
            ErrorKind::Shutdown(_) => None,
        }
    }
}

impl fmt::Display for ErrorKind {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ErrorKind::Bind(_, e) => write!(f, "binding failed: {e}"),
            ErrorKind::Io(e) => write!(f, "I/O error: {e}"),
            ErrorKind::Collisions(_) => "collisions detected".fmt(f),
            ErrorKind::FailedFairings(_) => "launch fairing(s) failed".fmt(f),
            ErrorKind::InsecureSecretKey(_) => "insecure secret key config".fmt(f),
            ErrorKind::Config(_) => "failed to extract configuration".fmt(f),
            ErrorKind::SentinelAborts(_) => "sentinel(s) aborted".fmt(f),
            ErrorKind::Liftoff(_, _) => "liftoff failed".fmt(f),
            ErrorKind::Shutdown(_) => "shutdown failed".fmt(f),
        }
    }
}

impl fmt::Debug for Error {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.kind.fmt(f)
    }
}

impl fmt::Display for Error {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.kind)
    }
}

impl fmt::Debug for Empty {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("empty parameter")
    }
}

impl fmt::Display for Empty {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("empty parameter")
    }
}

impl StdError for Empty { }

struct ServerError<'a>(&'a (dyn StdError + 'static));

impl fmt::Display for ServerError<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let error = &self.0;
        if let Some(e) = error.downcast_ref::<hyper::Error>() {
            write!(f, "request failed: {e}")?;
        } else if let Some(e) = error.downcast_ref::<io::Error>() {
            write!(f, "connection error: ")?;

            match e.kind() {
                io::ErrorKind::NotConnected => write!(f, "remote disconnected")?,
                io::ErrorKind::UnexpectedEof => write!(f, "remote sent early eof")?,
                io::ErrorKind::ConnectionReset
                | io::ErrorKind::ConnectionAborted => write!(f, "terminated by remote")?,
                _ => write!(f, "{e}")?,
            }
        } else {
            write!(f, "http server error: {error}")?;
        }

        Ok(())
    }
}

/// Log an error that occurs during request processing
#[track_caller]
pub(crate) fn log_server_error(error: &(dyn StdError + 'static)) {
    let mut error: &(dyn StdError + 'static) = error;
    if error.downcast_ref::<hyper::Error>().is_some() {
        warn_span!("minor server error" ["{}", ServerError(error)] => {
            while let Some(source) = error.source() {
                error = source;
                warn!("{}", ServerError(error));
            }
        });
    } else {
        error_span!("server error" ["{}", ServerError(error)] => {
            while let Some(source) = error.source() {
                error = source;
                error!("{}", ServerError(error));
            }
        });
    }
}
