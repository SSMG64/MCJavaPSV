use std::collections::VecDeque;
use std::fmt::{self, Write as _};
use std::path::Path;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use tracing::field::{Field, Visit};
use tracing::{Event, Level, Subscriber};
use tracing_appender::rolling::{Builder, Rotation};
use tracing_subscriber::layer::{Context, SubscriberExt};
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{EnvFilter, Layer};

const CONSOLE_CAPACITY: usize = 5_000;
const LOG_FILES_KEPT: usize = 7;

#[derive(Debug, Clone)]
pub struct ConsoleLine {
    pub level: Level,
    pub target: String,
    pub text: String,
    pub at: Duration,
}

impl ConsoleLine {
    pub fn render(&self) -> String {
        format!(
            "[{:>9.3}] {:<5} {}: {}",
            self.at.as_secs_f64(),
            self.level,
            self.target,
            self.text
        )
    }
}

/// A bounded in-memory copy of the log that the window's console shows.
#[derive(Debug, Default)]
pub struct ConsoleBuffer {
    lines: Mutex<VecDeque<ConsoleLine>>,
}

impl ConsoleBuffer {
    pub fn push(&self, line: ConsoleLine) {
        let mut lines = self.lines.lock().unwrap_or_else(PoisonError::into_inner);
        if lines.len() == CONSOLE_CAPACITY {
            lines.pop_front();
        }
        lines.push_back(line);
    }

    pub fn clear(&self) {
        self.lines
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clear();
    }

    pub fn len(&self) -> usize {
        self.lines
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn with_lines<R>(&self, f: impl FnOnce(&VecDeque<ConsoleLine>) -> R) -> R {
        f(&self.lines.lock().unwrap_or_else(PoisonError::into_inner))
    }

    pub fn to_text(&self) -> String {
        self.with_lines(|lines| {
            lines
                .iter()
                .map(ConsoleLine::render)
                .collect::<Vec<_>>()
                .join("\n")
        })
    }

    pub fn tail_text(&self, count: usize) -> String {
        self.with_lines(|lines| {
            let skip = lines.len().saturating_sub(count);
            lines
                .iter()
                .skip(skip)
                .map(ConsoleLine::render)
                .collect::<Vec<_>>()
                .join("\n")
        })
    }
}

#[derive(Default)]
struct FieldCollector {
    message: String,
    fields: String,
}

impl FieldCollector {
    fn finish(self) -> String {
        if self.fields.is_empty() {
            self.message
        } else if self.message.is_empty() {
            self.fields.trim_start().to_owned()
        } else {
            format!("{}{}", self.message, self.fields)
        }
    }
}

impl Visit for FieldCollector {
    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        if field.name() == "message" {
            let _ = write!(self.message, "{value:?}");
        } else {
            let _ = write!(self.fields, " {}={value:?}", field.name());
        }
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        if field.name() == "message" {
            self.message.push_str(value);
        } else {
            let _ = write!(self.fields, " {}={value}", field.name());
        }
    }
}

struct ConsoleLayer {
    buffer: Arc<ConsoleBuffer>,
    started: Instant,
}

impl<S: Subscriber> Layer<S> for ConsoleLayer {
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        let mut collector = FieldCollector::default();
        event.record(&mut collector);
        let metadata = event.metadata();
        self.buffer.push(ConsoleLine {
            level: *metadata.level(),
            target: metadata.target().to_owned(),
            text: collector.finish(),
            at: self.started.elapsed(),
        });
    }
}

#[derive(Debug, Clone)]
pub struct Logging {
    pub buffer: Arc<ConsoleBuffer>,
}

/// Installs the global subscriber: a rotating file in `log_dir`, the in-memory console
/// buffer, and optionally standard error. `RUST_LOG` overrides the default filter.
pub fn init(log_dir: &Path, also_stderr: bool, verbose: bool) -> Logging {
    let buffer = Arc::new(ConsoleBuffer::default());
    let default_filter = if verbose { "debug" } else { "info" };
    let filter =
        || EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(default_filter));

    let file_layer = std::fs::create_dir_all(log_dir)
        .ok()
        .and_then(|()| {
            Builder::new()
                .rotation(Rotation::DAILY)
                .filename_prefix("mcjavapsv")
                .filename_suffix("log")
                .max_log_files(LOG_FILES_KEPT)
                .build(log_dir)
                .ok()
        })
        .map(|appender| {
            tracing_subscriber::fmt::layer()
                .with_ansi(false)
                .with_writer(appender)
        });

    let stderr_layer = also_stderr.then(|| {
        tracing_subscriber::fmt::layer()
            .with_target(false)
            .with_writer(std::io::stderr)
    });

    let installed = tracing_subscriber::registry()
        .with(file_layer)
        .with(stderr_layer)
        .with(ConsoleLayer {
            buffer: Arc::clone(&buffer),
            started: Instant::now(),
        })
        .with(filter())
        .try_init();
    if installed.is_err() {
        eprintln!("logging was already initialised");
    }

    std::panic::set_hook(Box::new(|info| {
        tracing::error!(target: "panic", "{info}");
    }));

    Logging { buffer }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(text: &str) -> ConsoleLine {
        ConsoleLine {
            level: Level::INFO,
            target: "t".into(),
            text: text.into(),
            at: Duration::from_millis(1500),
        }
    }

    #[test]
    fn buffer_is_bounded() {
        let buffer = ConsoleBuffer::default();
        for i in 0..CONSOLE_CAPACITY + 10 {
            buffer.push(line(&i.to_string()));
        }
        assert_eq!(buffer.len(), CONSOLE_CAPACITY);
        assert_eq!(
            buffer.with_lines(|l| l.front().map(|x| x.text.clone())),
            Some("10".into())
        );
    }

    #[test]
    fn tail_and_clear() {
        let buffer = ConsoleBuffer::default();
        for text in ["a", "b", "c"] {
            buffer.push(line(text));
        }
        assert_eq!(buffer.tail_text(2).lines().count(), 2);
        assert!(buffer.tail_text(2).ends_with("c"));
        buffer.clear();
        assert!(buffer.is_empty());
    }

    #[test]
    fn render_has_time_level_and_target() {
        assert_eq!(line("hello").render(), "[    1.500] INFO  t: hello");
    }

    #[test]
    fn layer_captures_messages_and_fields() {
        let buffer = Arc::new(ConsoleBuffer::default());
        let subscriber = tracing_subscriber::registry().with(ConsoleLayer {
            buffer: Arc::clone(&buffer),
            started: Instant::now(),
        });
        tracing::subscriber::with_default(subscriber, || {
            tracing::warn!(path = "x", "something happened");
        });
        let text = buffer.to_text();
        assert!(text.contains("WARN"));
        assert!(text.contains("something happened path=x"));
    }
}
