// Instance status tracking
// Provides real-time status information for running instances
//
// v26.4-alpha.1 performance fix (alpha.6 bench finding): the all-instances
// path previously constructed a fresh `System::new_all()` + `refresh_all()`
// PER instance with a PID file — on large installs this dominated `mdl
// status` latency (p95 up to ~2s). A single shared process snapshot now
// serves every probe; `new_all` (disks/networks/users enumeration) is
// replaced by the much cheaper processes-only refresh.

use anyhow::{Result, Context};
use serde::{Deserialize, Serialize};
use sysinfo::{System, Pid, ProcessRefreshKind};

use super::{InstanceManager, Instance};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstanceStatusInfo {
    pub name: String,
    pub state: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pid: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uptime_seconds: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory_mb: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cpu_percent: Option<f32>,
}

pub struct InstanceStatus {
    manager: InstanceManager,
}

impl InstanceStatus {
    pub fn new() -> Result<Self> {
        Ok(Self {
            manager: InstanceManager::new()?,
        })
    }

    pub async fn get_instance_status(&self, name: &str) -> Result<InstanceStatusInfo> {
        let instance = self.manager.get(name).await?;
        let mut sys = System::new();
        sys.refresh_processes_specifics(ProcessRefreshKind::everything());
        Self::probe(&instance, &sys).await
    }

    pub async fn get_all_status(&self) -> Result<Vec<InstanceStatusInfo>> {
        let instances = self.manager.list().await?;

        // One process snapshot shared by every probe.
        let mut sys = System::new();
        sys.refresh_processes_specifics(ProcessRefreshKind::everything());

        let mut results = Vec::with_capacity(instances.len());
        for instance in &instances {
            results.push(Self::probe(instance, &sys).await?);
        }
        Ok(results)
    }

    /// Probe one instance against an existing process snapshot. Cleans up a
    /// stale PID file when the recorded process is gone.
    async fn probe(
        instance: &Instance,
        sys: &System,
    ) -> Result<InstanceStatusInfo> {
        let pid_file = instance.path.join("runtime").join("pid");

        let mut info = InstanceStatusInfo {
            name: instance.name.clone(),
            state: "stopped".to_string(),
            pid: None,
            uptime_seconds: None,
            memory_mb: None,
            cpu_percent: None,
        };

        if !pid_file.exists() {
            return Ok(info);
        }

        let pid_content = tokio::fs::read_to_string(&pid_file)
            .await
            .context("Failed to read PID file")?;

        let pid: u32 = pid_content.trim().parse()
            .context("Invalid PID format")?;

        if let Some(process) = sys.process(Pid::from_u32(pid)) {
            info.state = "running".to_string();
            info.pid = Some(pid);

            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs();
            info.uptime_seconds = Some(now.saturating_sub(process.start_time()));
            info.memory_mb = Some(process.memory() / 1024 / 1024);
            info.cpu_percent = Some(process.cpu_usage());
        } else {
            // PID file exists but process is dead - clean up
            let _ = tokio::fs::remove_file(&pid_file).await;
        }

        Ok(info)
    }
}
