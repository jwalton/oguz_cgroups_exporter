use crate::{
    matcher::{MatchableProcessConfig, NameMatcher},
    procs::{Proc, metrics::ProcessMetrics},
    render::MatchGroup,
};
use new_string_template::template::Template;
use procfs::process::Process;
use std::{collections::HashMap, result::Result};
use tokio::sync::mpsc;
use tokio_stream::{Stream, StreamExt as _, wrappers::ReceiverStream};
use tracing::{error, trace};

const NAMESPACE: &str = "process";

pub fn discover_procs_metrics(
    configs: &[MatchableProcessConfig],
) -> impl Stream<Item = MatchGroup<ProcessMetrics>> + 'static {
    let (send, recv) = mpsc::channel(10);
    let configs = configs.to_owned();
    std::thread::spawn(move || discover_thread(&configs, &send));
    let stream = ReceiverStream::new(recv);
    stream.filter_map(|match_group| match match_group {
        Ok(match_group) => Some(match_group),
        Err(err) => {
            error!(%err, cause =% err.root_cause(), "Error while discovering process metrics");
            None
        }
    })
}

fn discover_thread(
    configs: &[MatchableProcessConfig],
    sender: &mpsc::Sender<anyhow::Result<MatchGroup<ProcessMetrics>>>,
) {
    let mut groups = HashMap::new();
    let procs = match procfs::process::all_processes() {
        Ok(procs) => procs,
        Err(err) => {
            error!(%err, "Failed to get all processes");
            return;
        }
    };
    for process in procs.filter_map(Result::ok) {
        if let Err(err) = process_process(process, configs, &mut groups) {
            // Logging at the trace level to avoid cluttering the logs.
            trace!(%err, cause =% err.root_cause(), "Failed to process process");
        }
    }

    for (name, group) in groups {
        let (data, config) = group.into_parts();
        let metrics = ProcessMetrics::from_processes(data.into_iter(), &name);
        let match_group = MatchGroup::new(vec![metrics], config);
        let _ = sender.blocking_send(Ok(match_group));
    }
}

fn process_process(
    process: Process,
    configs: &[MatchableProcessConfig],
    groups: &mut HashMap<String, MatchGroup<Proc>>,
) -> anyhow::Result<()> {
    let process = Proc::try_from(process)?;
    for config in configs {
        let proc_value = process.value_for_matcher(&config.match_by);
        let group_name = config.match_by.name();
        if let Some(name) = config
            .match_by
            .matcher()
            .matching_group_name(proc_value, group_name, &process)
        {
            let group = groups.entry(name.clone()).or_insert_with(|| {
                let mut metrics_config = config.metrics.clone();
                if metrics_config.namespace.is_none() {
                    metrics_config.namespace = Some(NAMESPACE.to_string());
                }
                MatchGroup::new(vec![], metrics_config)
            });
            let mut process = process.clone();
            process.gather_remaining_info()?;
            group.insert(process);
        }
    }
    Ok(())
}

impl NameMatcher {
    fn matching_group_name(
        &self,
        proc_value: &str,
        group_name: &str,
        process: &Proc,
    ) -> Option<String> {
        match self {
            Self::Glob(pattern) => pattern.matches(proc_value).then(|| group_name.into()),
            Self::Regex(regex) => {
                if let Some(captures) = regex.captures(proc_value) {
                    let mut variables = init_variables_from_process(process);
                    for name in regex.capture_names() {
                        let Some(name) = name else { continue };
                        if let Some(matched) = captures.name(name) {
                            variables.insert(name.to_string(), matched.as_str().to_string());
                        }
                    }
                    let template = Template::new(group_name);
                    Some(template.render_nofail_string(&variables))
                } else {
                    None
                }
            }
        }
    }
}

fn init_variables_from_process(process: &Proc) -> HashMap<String, String> {
    let mut variables = HashMap::new();
    variables.insert("pid".into(), process.pid().to_string());
    variables.insert("exe".into(), process.exe().to_owned());
    variables.insert("comm".into(), process.comm().to_owned());
    variables
}
