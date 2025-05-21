use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use cgroups_exporter_config::load_config;
use notify::RecursiveMode;
use notify_debouncer_full::{DebouncedEvent, new_debouncer};

use tracing::{info, warn};

use crate::{matcher::MatchableConfig, server::SharedConfig};

const DEBOUNCE_TIMEOUT: Duration = Duration::from_secs(2);

pub async fn watch_config_file(config_file: PathBuf, config: SharedConfig) -> anyhow::Result<()> {
    let (send, mut recv) = tokio::sync::mpsc::channel(1);
    let dir_path = config_file.parent().unwrap_or_else(|| Path::new("."));
    let watch_path = dir_path.to_path_buf();

    // Although notify is not async, the following calls are not blocking.
    // Notify's API is weird.
    let target_path = config_file.clone();
    let mut debouncer = new_debouncer(DEBOUNCE_TIMEOUT, None, move |result| {
        if let Ok(events) = result {
            for event in events {
                let event: DebouncedEvent = event;
                if !matches!(
                    event.kind,
                    notify::EventKind::Create(_)
                        | notify::EventKind::Modify(_)
                        | notify::EventKind::Remove(_)
                ) {
                    continue;
                }
                for path in &event.paths {
                    if path == &target_path {
                        // We got a change event for the config file.
                        // Send a message to the channel.
                        send.blocking_send(()).ok();
                    }
                }
            }
        }
    })?;
    debouncer.watch(&watch_path, RecursiveMode::Recursive)?;

    while (recv.recv().await).is_some() {
        if let Err(err) = replace_shared_config(&config_file, &config).await {
            warn!(%err, "Failed to load config file");
        }
    }

    Ok(())
}

async fn replace_shared_config(file_path: &Path, config: &SharedConfig) -> anyhow::Result<()> {
    let contents = load_config(file_path).await?;
    let new_config: MatchableConfig = contents.try_into()?;
    info!(
        path =% file_path.display(),
        cgroups =% new_config.cgroups.len(),
        processes =% new_config.processes.len(),
        "Reloaded the config file"
    );
    config.update(Arc::new(new_config));
    Ok(())
}
