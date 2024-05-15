#[macro_use]
mod macros;
mod traceable;

#[cfg(feature = "trace")]
#[cfg_attr(nightly, doc(cfg(feature = "trace")))]
pub mod subscriber;

pub(crate) mod level;

#[doc(inline)]
pub use traceable::{Traceable, TraceableCollection};

#[doc(inline)]
pub use macros::*;

pub fn init<'a, T: Into<Option<&'a crate::Config>>>(_config: T) {
    #[cfg(all(feature = "trace", debug_assertions))]
    subscriber::RocketFmt::<subscriber::Pretty>::init(_config.into());

    #[cfg(all(feature = "trace", not(debug_assertions)))]
    subscriber::RocketFmt::<subscriber::Compact>::init(_config.into());
}
