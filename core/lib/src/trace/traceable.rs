use crate::fairing::Fairing;
use crate::{route, Catcher, Config, Response, Route};
use crate::util::Formatter;

use figment::Figment;
use rocket::http::Header;
use tracing::Level;

pub trait Traceable {
    fn trace(&self, level: Level);

    #[inline(always)] fn trace_info(&self) { self.trace(Level::INFO) }
    #[inline(always)] fn trace_warn(&self) { self.trace(Level::WARN) }
    #[inline(always)] fn trace_error(&self) { self.trace(Level::ERROR) }
    #[inline(always)] fn trace_debug(&self) { self.trace(Level::DEBUG) }
    #[inline(always)] fn trace_trace(&self) { self.trace(Level::TRACE) }
}

pub trait TraceableCollection: Sized {
    fn trace_all(self, level: Level);

    #[inline(always)] fn trace_all_info(self) { self.trace_all(Level::INFO) }
    #[inline(always)] fn trace_all_warn(self) { self.trace_all(Level::WARN) }
    #[inline(always)] fn trace_all_error(self) { self.trace_all(Level::ERROR) }
    #[inline(always)] fn trace_all_debug(self) { self.trace_all(Level::DEBUG) }
    #[inline(always)] fn trace_all_trace(self) { self.trace_all(Level::TRACE) }
}

impl<T: Traceable, I: IntoIterator<Item = T>> TraceableCollection for I {
    fn trace_all(self, level: Level) {
        self.into_iter().for_each(|i| i.trace(level))
    }
}

impl<T: Traceable> Traceable for &T {
    #[inline(always)]
    fn trace(&self, level: Level) {
        T::trace(self, level)
    }
}

impl Traceable for Figment {
    fn trace(&self, level: Level) {
        for param in Config::PARAMETERS {
            if let Some(source) = self.find_metadata(param) {
                event! { level, "figment",
                    param,
                    %source.name,
                    source.source = source.source.as_ref().map(display),
                }
            }
        }

        // Check for now deprecated config values.
        for (key, replacement) in Config::DEPRECATED_KEYS {
            if let Some(source) = self.find_metadata(key) {
                event! { Level::WARN, "deprecated",
                    key,
                    replacement,
                    %source.name,
                    source.source = source.source.as_ref().map(display),
                    "config key `{key}` is deprecated and has no meaning"
                }
            }
        }
    }
}

impl Traceable for Config {
    fn trace(&self, level: Level) {
        event! { level, "config",
            http2 = cfg!(feature = "http2"),
            log_level = self.log_level.map(|l| l.as_str()),
            cli_colors = %self.cli_colors,
            workers = self.workers,
            max_blocking = self.max_blocking,
            ident = %self.ident,
            ip_header = self.ip_header.as_ref().map(|s| s.as_str()),
            proxy_proto_header = self.proxy_proto_header.as_ref().map(|s| s.as_str()),
            limits = %Formatter(|f| f.debug_map()
                .entries(self.limits.limits.iter().map(|(k, v)| (k.as_str(), display(v))))
                .finish()),
            temp_dir = %self.temp_dir.relative().display(),
            keep_alive = (self.keep_alive != 0).then_some(self.keep_alive),
            shutdown.ctrlc = self.shutdown.ctrlc,
            shutdown.signals = %{
                #[cfg(not(unix))] {
                    "disabled (not unix)"
                }

                #[cfg(unix)] {
                    Formatter(|f| f.debug_set()
                        .entries(self.shutdown.signals.iter().map(|s| s.as_str()))
                        .finish())
                }
            },
                shutdown.grace = self.shutdown.grace,
                shutdown.mercy = self.shutdown.mercy,
                shutdown.force = self.shutdown.force,
        }

        #[cfg(feature = "secrets")] {
            if !self.secret_key.is_provided() {
                warn! {
                    name: "volatile_secret_key",
                    "secrets enabled without configuring a stable `secret_key`\n\
                    private/signed cookies will become unreadable after restarting\n\
                    disable the `secrets` feature or configure a `secret_key`\n\
                    this becomes a hard error in non-debug profiles",
                }
            }

            let secret_key_is_known = Config::KNOWN_SECRET_KEYS.iter().any(|&key_str| {
                let value = figment::value::Value::from(key_str);
                self.secret_key == value.deserialize().expect("known key is valid")
            });

            if secret_key_is_known {
                warn! {
                    name: "insecure_secret_key",
                    "The configured `secret_key` is exposed and insecure. \
                    The configured key is publicly published and thus insecure. \
                    Try generating a new key with `head -c64 /dev/urandom | base64`."
                }
            }
        }
    }
}

impl Traceable for Route {
    fn trace(&self, level: Level) {
        event! { level, "route",
            name = self.name.as_ref().map(|n| &**n),
            method = %self.method,
            rank = self.rank,
            uri = %self.uri,
            uri.base = %self.uri.base(),
            uri.unmounted = %self.uri.unmounted(),
            format = self.format.as_ref().map(display),
        }

        event! { Level::DEBUG, "route",
            route = self.name.as_ref().map(|n| &**n),
            sentinels = %Formatter(|f| {
                f.debug_set()
                    .entries(self.sentinels.iter().filter(|s| s.specialized).map(|s| s.type_name))
                    .finish()
            })
        }
    }
}

impl Traceable for Catcher {
    fn trace(&self, level: Level) {
        event! { level, "catcher",
            name = self.name.as_ref().map(|n| &**n),
            status = %Formatter(|f| match self.code {
                Some(code) => write!(f, "{}", code),
                None => write!(f, "default"),
            }),
            rank = self.rank,
            uri.base = %self.base(),
        }
    }
}

impl Traceable for &dyn Fairing {
    fn trace(&self, level: Level) {
        event!(level, "fairing", name = self.info().name, kind = %self.info().kind)
    }
}

impl Traceable for figment::error::Kind {
    fn trace(&self, _: Level) {
        use figment::error::{OneOf as V, Kind::*};

        match self {
            Message(message) => error!(message),
            InvalidType(actual, expected) => error!(name: "invalid type", %actual, expected),
            InvalidValue(actual, expected) => error!(name: "invalid value", %actual, expected),
            InvalidLength(actual, expected) => error!(name: "invalid length", %actual, expected),
            UnknownVariant(actual, v) => error!(name: "unknown variant", actual, expected = %V(v)),
            UnknownField(actual, v) => error!(name: "unknown field", actual, expected = %V(v)),
            UnsupportedKey(actual, v) => error!(name: "unsupported key", %actual, expected = &**v),
            MissingField(value) => error!(name: "missing field", value = &**value),
            DuplicateField(value) => error!(name: "duplicate field", value),
            ISizeOutOfRange(value) => error!(name: "out of range signed integer", value),
            USizeOutOfRange(value) => error!(name: "out of range unsigned integer", value),
            Unsupported(value) => error!(name: "unsupported type", %value),
        }
    }
}

impl Traceable for figment::Error {
    fn trace(&self, _: Level) {
        for e in self.clone() {
            let span = tracing::error_span! {
                "config",
                key = (!e.path.is_empty()).then_some(&e.path).and_then(|path| {
                    let (profile, metadata) = (e.profile.as_ref()?, e.metadata.as_ref()?);
                    Some(metadata.interpolate(profile, path))
                }),
                source.name = e.metadata.as_ref().map(|m| &*m.name),
                source.source = e.metadata.as_ref().and_then(|m| m.source.as_ref()).map(display),
            };

            span.in_scope(|| e.kind.trace_error());
        }
    }
}

impl Traceable for Header<'_> {
    fn trace(&self, level: Level) {
        event!(level, "header", name = self.name().as_str(), value = self.value());
    }
}

impl Traceable for route::Outcome<'_> {
    fn trace(&self, level: Level) {
        event!(level, "outcome",
            outcome = match self {
                Self::Success(..) => "success",
                Self::Error(..) => "error",
                Self::Forward(..) => "forward",
            },
            status = match self {
                Self::Success(r) => r.status().code,
                Self::Error(s) => s.code,
                Self::Forward((_, s)) => s.code,
            },
        )
    }
}

impl Traceable for Response<'_> {
    fn trace(&self, level: Level) {
        event!(level, "response", status = self.status().code);
    }
}
