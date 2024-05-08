pub trait PaintExt: Sized {
    fn emoji(self) -> yansi::Painted<Self>;
}

impl PaintExt for &str {
    /// Paint::masked(), but hidden on Windows due to broken output. See #1122.
    fn emoji(self) -> yansi::Painted<Self> {
        #[cfg(windows)] { yansi::Paint::new("").mask() }
        #[cfg(not(windows))] { yansi::Paint::new(self).mask() }
    }
}

macro_rules! declare_macro {
    ($($name:ident $level:ident),* $(,)?) => (
        $(declare_macro!([$] $name $level);)*
    );

    ([$d:tt] $name:ident $level:ident) => (
        #[macro_export]
        macro_rules! $name {
            ($d ($t:tt)*) => ({
                #[allow(unused_imports)]
                use $crate::trace::macros::PaintExt as _;
                $crate::tracing::$level!($d ($t)*);
            })
        }
    );
}

declare_macro!(
    // launch_meta INFO, launch_meta_ INFO,
    error error, error_ error,
    info info, info_ info,
    trace trace, trace_ trace,
    debug debug, debug_ debug,
    warn warn, warn_ warn,
);

macro_rules! declare_span_macro {
    ($($name:ident $level:ident),* $(,)?) => (
        $(declare_span_macro!([$] $name $level);)*
    );

    ([$d:tt] $name:ident $level:ident) => (
        #[macro_export]
        macro_rules! $name {
            ($n:literal $d ([ $d ($f:tt)* ])? => $in_scope:expr) => ({
                $crate::tracing::span!(tracing::Level::$level, $n $d (, $d ($f)* )?)
                    .in_scope(|| $in_scope);
            })
        }
    );
}

declare_span_macro!(info_span INFO, trace_span TRACE, debug_span DEBUG);

macro_rules! event {
    ($level:expr, $($args:tt)*) => {{
        match $level {
            $crate::tracing::Level::ERROR => event!(@$crate::tracing::Level::ERROR, $($args)*),
            $crate::tracing::Level::WARN => event!(@$crate::tracing::Level::WARN, $($args)*),
            $crate::tracing::Level::INFO => event!(@$crate::tracing::Level::INFO, $($args)*),
            $crate::tracing::Level::DEBUG => event!(@$crate::tracing::Level::DEBUG, $($args)*),
            $crate::tracing::Level::TRACE => event!(@$crate::tracing::Level::TRACE, $($args)*),
        }
    }};

    (@$level:expr, $n:expr, $($args:tt)*) => {{
        $crate::tracing::event!(name: $n, target: concat!("rocket::", $n), $level, $($args)*);
    }};
}
