// Agent events

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AgentEvent {
    InstanceCreated { name: String },
    InstanceDeleted { name: String },
    InstanceLaunched { name: String },
    InstanceStopped { name: String },
    InstanceCrashed { name: String },
}
