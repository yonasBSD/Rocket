use std::fmt;
use std::sync::OnceLock;

use tracing::{Event, Level, Metadata, Subscriber};
use tracing::span::{Attributes, Id, Record};
use tracing::field::Field;

use tracing_subscriber::layer::{Layer, Context};
use tracing_subscriber::registry::LookupSpan;
use tracing_subscriber::field::RecordFields;

use yansi::{Paint, Painted};

use crate::trace::subscriber::{Data, RecordDisplay, Handle, RocketFmt};
use crate::util::Formatter;

#[derive(Debug, Default, Copy, Clone)]
pub struct Pretty {
    depth: u32,
}

impl RocketFmt<Pretty> {
    pub fn init(config: Option<&crate::Config>) {
        static HANDLE: OnceLock<Handle<Pretty>> = OnceLock::new();

        Self::init_with(config, &HANDLE);
    }

    fn indent(&self) -> &'static str {
        static INDENT: &[&str] = &["", "   ", "      "];
        INDENT.get(self.state().depth as usize).copied().unwrap_or("         ")
    }

    fn marker(&self) -> &'static str {
        static MARKER: &[&str] = &["", ">> ", ":: "];
        MARKER.get(self.state().depth as usize).copied().unwrap_or("-- ")
    }

    fn emoji(&self, emoji: &'static str) -> Painted<&'static str> {
        #[cfg(windows)] { "".paint(self.style).mask() }
        #[cfg(not(windows))] { emoji.paint(self.style).mask() }
    }

    fn prefix<'a>(&self, meta: &'a Metadata<'_>) -> impl fmt::Display + 'a {
        let (i, m, s) = (self.indent(), self.marker(), self.style(meta));
        Formatter(move |f| match *meta.level() {
            Level::WARN => write!(f, "{i}{m}{} ", "warning:".paint(s).bold()),
            Level::ERROR => write!(f, "{i}{m}{} ", "error:".paint(s).bold()),
            Level::INFO => write!(f, "{i}{m}"),
            level => write!(f, "{i}{m}[{} {}] ", level.paint(s).bold(), meta.target()),
        })
    }

    fn print_pretty<F: RecordFields>(&self, m: &Metadata<'_>, data: F) {
        let prefix = self.prefix(m);
        let cont_prefix = Formatter(|f| {
            let style = self.style(m);
            write!(f, "{}{} ", self.indent(), "++".paint(style).dim())
        });

        self.print(&prefix, &cont_prefix, m, data);
    }

    fn print_fields<F>(&self, metadata: &Metadata<'_>, fields: F)
        where F: RecordFields
    {
        let style = self.style(metadata);
        let prefix = self.prefix(metadata);
        fields.record_display(|key: &Field, value: &dyn fmt::Display| {
            if key.name() != "message" {
                println!("{prefix}{}: {}", key.paint(style), value.paint(style).primary());
            }
        })
    }
}

impl<S: Subscriber + for<'a> LookupSpan<'a>> Layer<S> for RocketFmt<Pretty> {
    fn enabled(&self, metadata: &Metadata<'_>, _: Context<'_, S>) -> bool {
        self.filter.would_enable(metadata.target(), metadata.level())
    }

    fn on_event(&self, event: &Event<'_>, _: Context<'_, S>) {
        let (meta, data) = (event.metadata(), Data::new(event));
        let style = self.style(meta);
        match meta.name() {
            "config" => self.print_fields(meta, event),
            "liftoff" => {
                let prefix = self.prefix(meta);
                println!("{prefix}{}{} {}", self.emoji("🚀 "),
                    "Rocket has launched from".paint(style).primary().bold(),
                    &data["endpoint"].paint(style).primary().bold().underline());
            }
            _ => self.print_pretty(meta, event),
        }
    }

    fn on_new_span(&self, attrs: &Attributes<'_>, id: &Id, ctxt: Context<'_, S>) {
        let data = Data::new(attrs);
        let span = ctxt.span(id).expect("new_span: span does not exist");
        if &data["count"] != "0" {
            let name = span.name();
            let icon = match name {
                "config" => "🔧 ",
                "routes" => "📬 ",
                "catchers" => "🚧 ",
                "fairings" => "📦 ",
                "shield" => "🛡️ ",
                "request" => "● ",
                _ => "",
            };

            let meta = span.metadata();
            let style = self.style(meta);
            let prefix = self.prefix(meta);
            let emoji = self.emoji(icon);
            let name = name.paint(style).bold();

            if !attrs.fields().is_empty() {
                println!("{prefix}{emoji}{name} ({})", self.compact_fields(meta, attrs))
            } else {
                println!("{prefix}{emoji}{name}");
            }
        }

        span.extensions_mut().replace(data);
    }

    fn on_record(&self, id: &Id, values: &Record<'_>, ctxt: Context<'_, S>) {
        let span = ctxt.span(id).expect("new_span: span does not exist");
        match span.extensions_mut().get_mut::<Data>() {
            Some(data) => values.record(data),
            None => span.extensions_mut().insert(Data::new(values)),
        }

        let meta = span.metadata();
        println!("{}{}", self.prefix(meta), self.compact_fields(meta, values));
    }

    fn on_enter(&self, _: &Id, _: Context<'_, S>) {
        self.update_state(|state| state.depth = state.depth.saturating_add(1));
    }

    fn on_exit(&self, _: &Id, _: Context<'_, S>) {
        self.update_state(|state| state.depth = state.depth.saturating_sub(1));
    }
}
