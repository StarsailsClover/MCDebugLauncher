// OptiFine installer
// Note: OptiFine installation requires manual JAR download due to licensing restrictions

use anyhow::Result;
use std::path::Path;

pub struct OptiFineInstaller {
    version: Option<String>,
}

impl OptiFineInstaller {
    pub fn new(version: Option<String>) -> Self {
        Self { version }
    }

    pub async fn install_loader(&self, mc_version: &str, _optifine_version: &str, target_dir: &Path) -> Result<String> {
        // OptiFine cannot be automatically downloaded due to licensing restrictions
        let message = format!(
            "\nOptiFine installation requires manual steps:\n\
            1. Download OptiFine for Minecraft {} from https://optifine.net/downloads\n\
            2. Place the OptiFine JAR in: {}/\n\
            3. Run the launcher again\n\n\
            OptiFine cannot be automatically downloaded due to licensing restrictions.",
            mc_version, target_dir.display()
        );

        anyhow::bail!("{}", message)
    }
}

