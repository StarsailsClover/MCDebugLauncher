// Instance status tracking
// Provides real-time status information for running instances

use anyhow::{Result, Context};
use serde::{Serialize, Deserialize};
use sysinfo::{System, Pid};

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
    system: System,
}

impl InstanceStatus {
    pub fn new() -> Result<Self> {
        Ok(Self {
            manager: InstanceManager::new()?,
            system: System::new_all(),
        })
    }

    pub async fn get_instance_status(&self, name: &str) -> Result<InstanceStatusInfo> {
        let instance = self.manager.get(name).await?;
        self.get_status_for_instance(&instance).await
    }

    pub async fn get_all_status(&self) -> Result<Vec<InstanceStatusInfo>> {
        let instances = self.manager.list().await?;
        let mut results = Vec::new();

        for instance in instances {
            let status = self.get_status_for_instance(&instance).await?;
            results.push(status);
        }

        Ok(results)
    }

    async fn get_status_for_instance(&self, instance: &Instance) -> Result<InstanceStatusInfo> {
        let pid_file = instance.path.join("runtime").join("pid");

        let mut info = InstanceStatusInfo {
            name: instance.name.clone(),
            state: "stopped".to_string(),
            pid: None,
            uptime_seconds: None,
            memory_mb: None,
            cpu_percent: None,
        };

        // Check if PID file exists
        if !pid_file.exists() {
            return Ok(info);
        }

        // Read PID
        let pid_content = tokio::fs::read_to_string(&pid_file)
            .await
            .context("Failed to read PID file")?;

        let pid: u32 = pid_content.trim().parse()
            .context("Invalid PID format")?;

        // Check if process is actually running
        let mut sys = System::new_all();
        sys.refresh_all();

        if let Some(process) = sys.process(Pid::from_u32(pid)) {
            info.state = "running".to_string();
            info.pid = Some(pid);

            // Get uptime
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs();
            let start_time = process.start_time();
            info.uptime_seconds = Some(now - start_time);

            // Get memory usage (in MB)
            info.memory_mb = Some(process.memory() / 1024 / 1024);

            // Get CPU usage
            info.cpu_percent = Some(process.cpu_usage());
        } else {
            // PID file exists but process is dead - clean up
            let _ = tokio::fs::remove_file(&pid_file).await;
        }

        Ok(info)
    }
}
