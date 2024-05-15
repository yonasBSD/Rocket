#[macro_use]
pub mod macros;
#[cfg(feature = "trace")]
pub mod subscriber;
pub mod level;
pub mod traceable;

pub use traceable::Traceable;

pub fn init<'a, T: Into<Option<&'a crate::Config>>>(_config: T) {
    #[cfg(all(feature = "trace", debug_assertions))]
    subscriber::RocketFmt::<subscriber::Pretty>::init(_config.into());

    #[cfg(all(feature = "trace", not(debug_assertions)))]
    subscriber::RocketFmt::<subscriber::Compact>::init(_config.into());
}
