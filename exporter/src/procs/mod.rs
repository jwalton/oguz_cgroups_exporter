mod discover;
mod metrics;

use derive_getters::Getters;
pub use discover::discover_procs_metrics;
pub use metrics::ProcessMetrics;
use procfs::ProcResult;

use crate::matcher::ProcessMatcher;

/// A wrapper around `procfs::process::Process` for testability and caching. Cloneable.

#[derive(Debug, Clone, Getters)]
pub struct Proc {
    pid: i32,
    exe: String,
    exe_base: String,
    cmdline: String,
    stat: procfs::process::Stat,
    io: Option<procfs::process::Io>,
    fd_count: Option<usize>,
}

impl TryFrom<procfs::process::Process> for Proc {
    type Error = procfs::ProcError;

    fn try_from(process: procfs::process::Process) -> Result<Self, Self::Error> {
        let stat = process.stat()?;
        let exe_path = process.exe().unwrap_or_default();
        let exe = exe_path.display().to_string();
        let exe_base = exe_path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        let cmdline = process.cmdline()?.join(" ");
        Ok(Self {
            pid: process.pid(),
            exe,
            exe_base,
            cmdline,
            stat,
            // These fields will be gathered later once a process passes the matcher.
            io: None,
            fd_count: None,
        })
    }
}

impl Proc {
    #[allow(dead_code)]
    pub(crate) fn new(
        pid: i32,
        exe: String,
        cmdline: String,
        stat: procfs::process::Stat,
        io: Option<procfs::process::Io>,
        fd_count: Option<usize>,
    ) -> Self {
        let exe_base = exe.rsplit('/').next().unwrap_or(&exe).to_string();
        Self {
            pid,
            exe,
            exe_base,
            cmdline,
            stat,
            io,
            fd_count,
        }
    }

    pub fn value_for_matcher(&self, proc_matcher: &ProcessMatcher) -> &str {
        match proc_matcher {
            ProcessMatcher::Exe { .. } => self.exe(),
            ProcessMatcher::ExeBase { .. } => self.exe_base(),
            ProcessMatcher::Comm { .. } => self.comm(),
            ProcessMatcher::Cmdline { .. } => self.cmdline(),
        }
    }

    pub fn comm(&self) -> &str {
        &self.stat.comm
    }

    pub fn gather_remaining_info(&mut self) -> ProcResult<()> {
        self.io = Some(procfs::process::Process::new(self.pid)?.io()?);
        self.fd_count = Some(procfs::process::Process::new(self.pid)?.fd_count()?);
        Ok(())
    }
}
