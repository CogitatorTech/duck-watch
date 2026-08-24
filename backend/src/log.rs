use std::str::FromStr;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::{EnvFilter, Registry, fmt::Layer, prelude::*};

pub fn init(level: &str, pretty: bool) -> WorkerGuard {
    let (non_blocking_io, guard) = tracing_appender::non_blocking(std::io::stdout());

    let json_log = match pretty {
        false => Some(Layer::default().with_writer(non_blocking_io).json()),
        true => None,
    };

    let pretty_log = match pretty {
        true => Some(Layer::default().pretty()),
        false => None,
    };

    let env_filter = EnvFilter::from_str(level).unwrap_or_else(|_| EnvFilter::new("info"));

    Registry::default()
        .with(json_log)
        .with(pretty_log)
        .with(env_filter)
        .init();

    guard
}
