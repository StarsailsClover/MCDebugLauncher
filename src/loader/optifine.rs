// OptiFine installer
// Note: OptiFine installation requires manual JAR download due to licensing restrictions
// This installer provides guidance for manual installation

use anyhow::Result;

pub struct OptiFineInstaller {
    version: String,
}

impl OptiFineInstaller {
    pub fn new(version: String) -> Self {
        Self { version }
    }
}

impl crate::loader::LoaderInstaller for OptiFineInstaller {
    fn install(&self, mc_version: &str, target_dir: &str) -> Result<String> {
        // OptiFine cannot be automatically downloaded due to licensing restrictions
        // Users must manually download from optifine.net
        let message = format!(
            "\nOptiFine installation requires manual steps:\n\
            1. Download OptiFine {} for Minecraft {} from https://optifine.net/downloads\n\
            2. Place the OptiFine JAR in: {}/\n\
            3. Run the launcher again\n\n\
            OptiFine cannot be automatically downloaded due to licensing restrictions.",
            self.version, mc_version, target_dir
        );

        anyhow::bail!("{}", message)
    }

    fn version(&self) -> &str {
        &self.version
    }

    fn loader_type(&self) -> &str {
        "optifine"
    }
}
