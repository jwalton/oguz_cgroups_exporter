use std::{
    collections::HashMap,
    num::NonZero,
    process::Command,
    sync::{Arc, Mutex},
};

use cgroups_exporter_config::ShellCommandStream;
use lru::LruCache;
#[cfg(test)]
use mockall::automock;
use new_string_template::template::Template;
use shell_quote::Sh;
use tokio::sync::broadcast;
use tracing::trace;

#[derive(Debug, Clone)]
pub struct ShellEvaluator {
    shared: Shared,
}

#[derive(Debug, Clone)]
struct Shared {
    state: Arc<Mutex<State>>,
}

#[derive(Debug)]
struct State {
    in_progress: HashMap<TaskId, broadcast::Receiver<Result<String, String>>>,
    completed: LruCache<TaskId, String>,
}

#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    Hash,
    derive_more::From,
    derive_more::Into,
    derive_more::AsRef,
    derive_more::Deref,
    derive_more::Display,
)]
struct TaskId {
    command: String,
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Failed to prepare shell command: {0}")]
    TemplateRender(#[from] new_string_template::error::TemplateError),
    #[error("Failed to execute shell command: {0}")]
    Execution(std::io::Error),
    #[error("Failed to wait on ongoing shell command execution: {0}")]
    Wait(String),
    #[error("Shell command returned empty result")]
    Empty,
    #[error("The command exited with code: {0}")]
    Exit(i32),
    #[error("Evaluator shutting down")]
    ShuttingDown,
}

enum CommandState {
    NotStarted,
    InProgress(broadcast::Receiver<Result<String, String>>),
    Completed(String),
}

#[cfg_attr(test, automock)]
pub trait Evaluator {
    /// Evaluates a shell command template with the given variables.
    // mockall needs the explicit lifetime
    #[allow(clippy::needless_lifetimes)]
    fn evaluate_blocking<'k>(
        &self,
        command_tpl: &str,
        variables: HashMap<&'k str, String>,
        output: ShellCommandStream,
    ) -> Result<String, Error>;
}

impl Evaluator for ShellEvaluator {
    fn evaluate_blocking(
        &self,
        command_tpl: &str,
        variables: HashMap<&str, String>,
        output: ShellCommandStream,
    ) -> Result<String, Error> {
        let prepared_command = prepare_shell_command(command_tpl, variables);
        match self.shared.get_command_state(&prepared_command) {
            CommandState::Completed(result) => Ok(result),
            CommandState::InProgress(mut receiver) => {
                // The error cases are closed, and lagged. Lagged shouldn't happen.
                receiver
                    .blocking_recv()
                    .map_err(|_| Error::ShuttingDown)?
                    .map_err(Error::Wait)
            }
            CommandState::NotStarted => {
                let (sender, receiver) = broadcast::channel(1);
                {
                    let mut state = self.shared.state.lock().unwrap();
                    state.in_progress.insert(prepared_command.clone(), receiver);
                }
                match Self::evaluate_inner(&prepared_command, output) {
                    Ok(result) => {
                        let result = result.trim().to_string();
                        trace!(output =% result, "Successful shell command execution");
                        if result.is_empty() {
                            sender.send(Err(Error::Empty.to_string())).ok();
                            return Err(Error::Empty);
                        }
                        {
                            let mut state = self.shared.state.lock().unwrap();
                            state
                                .completed
                                .put(prepared_command.clone(), result.clone());
                            state.in_progress.remove(&prepared_command);
                        }
                        sender.send(Ok(result.clone())).ok();
                        Ok(result)
                    }
                    Err(err) => {
                        {
                            let mut state = self.shared.state.lock().unwrap();
                            state.in_progress.remove(&prepared_command);
                        }
                        sender.send(Err(err.to_string())).ok();
                        Err(err)
                    }
                }
            }
        }
    }
}

impl ShellEvaluator {
    pub fn new(cache_capacity: usize) -> Self {
        let cache_capacity = NonZero::new(cache_capacity).unwrap_or(NonZero::new(100).unwrap());
        let shared = Arc::new(Mutex::new(State {
            in_progress: HashMap::new(),
            completed: LruCache::new(cache_capacity),
        }));

        ShellEvaluator {
            shared: Shared { state: shared },
        }
    }

    fn evaluate_inner(task: &TaskId, output: ShellCommandStream) -> Result<String, Error> {
        let command = Command::new("sh")
            .arg("-c")
            .arg(&task.command)
            .output()
            .map_err(Error::Execution)?;

        if !command.status.success() {
            return Err(Error::Exit(command.status.code().unwrap_or(-1)));
        }
        match output {
            ShellCommandStream::Stdout => {
                let stdout = String::from_utf8_lossy(&command.stdout);
                Ok(stdout.to_string())
            }
            ShellCommandStream::Stderr => {
                let stderr = String::from_utf8_lossy(&command.stderr);
                Ok(stderr.to_string())
            }
        }
    }
}

impl Shared {
    fn get_command_state(&self, command: &TaskId) -> CommandState {
        let mut state = self.state.lock().unwrap();
        if let Some(result) = state.completed.get(command) {
            CommandState::Completed(result.clone())
        } else if let Some(receiver) = state.in_progress.get(command) {
            CommandState::InProgress(receiver.resubscribe())
        } else {
            CommandState::NotStarted
        }
    }
}

fn prepare_shell_command(command_tpl: &str, variables: HashMap<&str, String>) -> TaskId {
    let template = Template::new(command_tpl);
    let variables = variables
        .into_iter()
        .map(|(key, value)| {
            let bytes = Sh::quote_vec(&value);
            let quoted = String::from_utf8_lossy(&bytes).to_string();
            (key, quoted)
        })
        .collect::<HashMap<_, _>>();

    let command = template.render_nofail(&variables);
    command.into()
}
