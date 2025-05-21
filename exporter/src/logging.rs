use std::{env, str::FromStr as _};

use tracing_subscriber::EnvFilter;

use strum::{AsRefStr, Display, EnumString};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, EnumString, Display, AsRefStr)]
#[strum(serialize_all = "lowercase")]
pub enum LogFormat {
    #[default]
    Pretty,
    Json,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, EnumString, Display, AsRefStr)]
#[strum(serialize_all = "lowercase")]
pub enum LogLevel {
    Trace,
    Debug,
    #[default]
    Info,
    Warn,
    Error,
}

pub fn setup_logging(
    log_level: Option<LogLevel>,
    log_format: Option<LogFormat>,
) -> Result<(), Box<dyn std::error::Error>> {
    let log_level = log_level.unwrap_or_default();
    let level_str = log_level.as_ref();
    let log_level = tracing::Level::from_str(level_str)
        .map_err(|_| format!("Invalid log level {level_str}"))?;
    let env_filter = EnvFilter::new(env::var("LOG_FILTER").unwrap_or_else(|_| {
        format!("{log_level},tower_http=warn,hyper=warn,tokio_util=warn,axum=warn,notify=warn,notify_debouncer_full=warn")
    }));

    match log_format.unwrap_or_default() {
        LogFormat::Pretty => {
            tracing_subscriber::fmt()
                .with_max_level(log_level)
                .with_env_filter(env_filter)
                .init();
        }
        LogFormat::Json => {
            tracing_subscriber::fmt()
                .json()
                .flatten_event(true)
                .with_max_level(log_level)
                .with_env_filter(env_filter)
                .init();
        }
    }

    Ok(())
}

/// Sets up a panic hook that logs panics as an error message, then exits.
pub fn set_panic_hook() {
    std::panic::set_hook(Box::new(|panic_info| {
        let message = panic_info
            .payload()
            .downcast_ref::<&str>()
            .copied()
            .unwrap_or_else(|| {
                panic_info
                    .payload()
                    .downcast_ref::<String>()
                    .map_or("<unknown>", |s| &**s)
            });
        let location = panic_info.location().unwrap();
        let file = location.file();
        let line = location.line();
        let column = location.column();
        let thread = std::thread::current();
        let thread_name = thread.name().unwrap_or("<unnamed>");
        let thread_id = thread.id();
        tracing::error!(
            thread_name,
            ?thread_id,
            message,
            file,
            line,
            column,
            "thread panicked",
        );
        std::process::exit(1);
    }));
}
