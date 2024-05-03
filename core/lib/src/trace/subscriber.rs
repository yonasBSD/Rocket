use std::marker::PhantomData;
use std::ops::Index;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU8, Ordering};
use std::fmt::{self, Debug, Display};
// use std::time::Instant;

use tracing::{Event, Level, Metadata, Subscriber};
use tracing::level_filters::LevelFilter;
use tracing::field::{Field, Visit};
use tracing::span::{Attributes, Id};

use tracing_subscriber::prelude::*;
use tracing_subscriber::layer::Context;
use tracing_subscriber::registry::LookupSpan;
use tracing_subscriber::{reload, filter, Layer, Registry};
use tracing_subscriber::field::RecordFields;

use figment::Source::File as RelPath;
use yansi::{Condition, Paint, Painted, Style};
use tinyvec::TinyVec;

use crate::config::{Config, CliColors};
use crate::util::Formatter;

pub trait PaintExt: Sized {
    fn emoji(self) -> Painted<Self>;
}

impl PaintExt for &str {
    /// Paint::masked(), but hidden on Windows due to broken output. See #1122.
    fn emoji(self) -> Painted<Self> {
        #[cfg(windows)] { Paint::new("").mask() }
        #[cfg(not(windows))] { Paint::new(self).mask() }
    }
}

pub(crate) fn init(config: Option<&Config>) {
    static HANDLE: OnceLock<reload::Handle<RocketFmt<Registry>, Registry>> = OnceLock::new();

    // Do nothing if there's no config and we've already initialized.
    if config.is_none() && HANDLE.get().is_some() {
        return;
    }

    let cli_colors = config.map(|c| c.cli_colors).unwrap_or(CliColors::Auto);
    let log_level = config.map(|c| c.log_level).unwrap_or(Some(Level::INFO));
    let (layer, reload_handle) = reload::Layer::new(RocketFmt::new(cli_colors, log_level));
    let result = tracing_subscriber::registry()
        .with(layer)
        .try_init();

    if result.is_ok() {
        assert!(HANDLE.set(reload_handle).is_ok());
    } if let Some(handle) = HANDLE.get() {
        assert!(handle.modify(|layer| layer.set(cli_colors, log_level)).is_ok());
    }
}

pub(crate) struct Data {
    // start: Instant,
    map: TinyVec<[(&'static str, String); 2]>,
}

impl Data {
    pub fn new<T: RecordFields>(attrs: &T) -> Self {
        let mut data = Data {
            // start: Instant::now(),
            map: TinyVec::new(),
        };

        attrs.record(&mut data);
        data
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.map.iter()
            .find(|(k, _)| k == &key)
            .map(|(_, v)| v.as_str())
    }
}

impl Index<&str> for Data {
    type Output = str;

    fn index(&self, index: &str) -> &Self::Output {
        self.get(index).unwrap_or("[internal error: missing key]")
    }
}

impl Visit for Data {
    fn record_debug(&mut self, field: &Field, value: &dyn Debug) {
        self.map.push((field.name(), format!("{:?}", value)));
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        self.map.push((field.name(), value.into()));
    }
}

#[derive(Default)]
struct RocketFmt<S> {
    depth: AtomicU8,
    filter: filter::Targets,
    default_style: Style,
    _subscriber: PhantomData<fn() -> S>
}

// struct Printer {
//     level: Level,
// }
//
// impl Printer {
//     fn print(event: &Event)
//
// }

macro_rules! log {
    ($this:expr, $event:expr => $fmt:expr $(, $($t:tt)*)?) => {
        let metadata = $event.metadata();
        let (i, s, t) = ($this.indent(), $this.style($event), metadata.target());
        match *metadata.level() {
            Level::WARN => print!(
                concat!("{}{} ", $fmt),
                i, "warning:".paint(s).yellow().bold() $(, $($t)*)?
            ),
            Level::ERROR => print!(
                concat!("{}{} ", $fmt),
                i, "error:".paint(s).red().bold() $(, $($t)*)?
            ),
            level@(Level::DEBUG | Level::TRACE) => match (metadata.file(), metadata.line()) {
                (Some(f), Some(l)) => print!(
                    concat!("{}[{} {}{}{} {}] ", $fmt),
                    i, level.paint(s).bold(),
                    RelPath(f.into()).underline(), ":".paint(s).dim(), l, t.paint(s).dim()
                    $(, $($t)*)?
                ),
                (_, _) => print!(
                    concat!("{}[{} {}] ", $fmt),
                    i, level.paint(s).bold(), t $(, $($t)*)?
                ),
            }
            _ => print!(concat!("{}", $fmt), i $(, $($t)*)?),
        }
    };
}

macro_rules! logln {
    ($this:expr, $event:expr => $fmt:literal $($t:tt)*) => {
        log!($this, $event => concat!($fmt, "\n") $($t)*);
    };
}

struct DisplayVisit<F>(F);

impl<F: FnMut(&Field, &dyn fmt::Display)> Visit for DisplayVisit<F> {
    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        (self.0)(field, &Formatter(|f| value.fmt(f)));
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        (self.0)(field, &value)
    }
}

trait DisplayFields {
    fn record_display<F: FnMut(&Field, &dyn fmt::Display)>(&self, f: F);
}

impl<T: RecordFields> DisplayFields for T {
    fn record_display<F: FnMut(&Field, &dyn fmt::Display)>(&self, f: F) {
        self.record(&mut DisplayVisit(f));
    }
}

impl<S: Subscriber + for<'a> LookupSpan<'a>> RocketFmt<S> {
    fn new(cli_colors: CliColors, level: impl Into<LevelFilter>) -> Self {
        let mut this = Self {
            depth: AtomicU8::new(0),
            filter: filter::Targets::new(),
            default_style: Style::new(),
            _subscriber: PhantomData,
        };

        this.set(cli_colors, level.into());
        this
    }

    fn set(&mut self, cli_colors: CliColors, level: impl Into<LevelFilter>) {
        self.default_style = Style::new().whenever(match cli_colors {
            CliColors::Always => Condition::ALWAYS,
            CliColors::Auto => Condition::DEFAULT,
            CliColors::Never => Condition::NEVER,
        });

        self.filter = filter::Targets::new()
            .with_default(level.into())
            .with_target("rustls", LevelFilter::OFF)
            .with_target("hyper", LevelFilter::OFF);
    }

    fn indent(&self) -> &'static str {
        match self.depth.load(Ordering::Acquire) {
            0 => "",
            1 => "    >> ",
            2 => "        >> ",
            _ => "            >> ",
        }
    }

    fn style(&self, event: &Event<'_>) -> Style {
        match *event.metadata().level() {
            Level::ERROR => self.default_style.red(),
            Level::WARN => self.default_style.yellow(),
            Level::INFO => self.default_style.blue(),
            Level::DEBUG => self.default_style.green(),
            Level::TRACE => self.default_style.magenta(),
        }
    }

    fn print(&self, event: &Event<'_>) {
        let style = self.style(event);
        let fields = event.metadata().fields();
        if let Some(msg) = fields.field("message") {
            event.record_display(|field: &Field, value: &dyn Display| {
                if field == &msg {
                    log!(self, event => "{}", value.paint(style));
                }
            });

            if fields.len() > 1 { print!(" ("); }
            self.print_fields_compact(false, event);
            if fields.len() > 1 { print!(")"); }
        } else if !fields.is_empty() {
            self.print_fields_compact(true, event);
        }

        if !fields.is_empty() {
            println!("");
        }
    }

    fn print_fields_compact(&self, prefix: bool, event: &Event<'_>) {
        let key_style = self.style(event).bold();
        let val_style = self.style(event).primary();
        let mut printed = false;
        event.record_display(|field: &Field, val: &dyn Display| {
            let key = field.name();
            if key != "message" {
                if !printed && prefix {
                    log!(self, event => "{}: {}", key.paint(key_style), val.paint(val_style));
                } else {
                    if printed { print!(" "); }
                    print!("{}: {}", key.paint(key_style), val.paint(val_style));
                }

                printed = true;
            }
        });
    }

    fn print_fields(&self, event: &Event<'_>) {
        let style = self.style(event);
        event.record_display(|key: &Field, value: &dyn Display| {
            if key.name() != "message" {
                logln!(self, event => "{}: {}", key.paint(style), value.paint(style).primary());
            }
        })
    }

    fn write_config(&self, event: &Event<'_>) {
        // eprintln!("  > config [name = {}]", event.metadata().name());
        match event.metadata().name() {
            "values" => self.print_fields(event),
            _ => self.print(event),
        }
    }
}

impl<S: Subscriber + for<'a> LookupSpan<'a>> Layer<S> for RocketFmt<S> {
    fn enabled(&self, metadata: &Metadata<'_>, _: Context<'_, S>) -> bool {
        self.filter.would_enable(metadata.target(), metadata.level())
    }

    fn on_event(&self, event: &Event<'_>, ctxt: Context<'_, S>) {
        // let metadata = event.metadata();
        // eprintln!("[name = {}, target = {}]", metadata.name(), metadata.target());
        if let Some(span) = ctxt.event_span(event) {
            // eprintln!("  > [name = {}, target = {}]", span.name(), span.metadata().target());
            return match span.name() {
                "config" => self.write_config(event),
                _ => self.print(event),
            };
        }

        self.print(event);
    }

    fn on_new_span(&self, attrs: &Attributes<'_>, id: &Id, ctx: Context<'_, S>) {
        let span = ctx.span(id).expect("new_span: span does not exist");
        let data = Data::new(attrs);
        match span.metadata().name() {
            "config" => println!("configured for {}", &data["profile"]),
            name => println!("{name} {:?}", Formatter(|f| {
                f.debug_map().entries(data.map.iter().map(|(k, v)| (k, v))).finish()
            }))
        }

        span.extensions_mut().replace(data);
    }

    fn on_enter(&self, _: &Id, _: Context<'_, S>) {
        self.depth.fetch_add(1, Ordering::AcqRel);
        // let metadata = ctxt.span(id).unwrap().metadata();
        // eprintln!("enter [name={}] [target={}] {:?}", metadata.name(),
        // metadata.target(), metadata.fields());
    }

    fn on_exit(&self, _: &Id, _: Context<'_, S>) {
        self.depth.fetch_sub(1, Ordering::AcqRel);
        // let metadata = ctxt.span(id).unwrap().metadata();
        // eprintln!("exit [name={}] [target={}] {:?}", metadata.name(),
        // metadata.target(), metadata.fields());
    }
}
