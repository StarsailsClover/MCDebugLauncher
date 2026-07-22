// Mod loader management module
// Handles installation of various mod loaders

pub mod fabric;
pub mod forge;
pub mod neoforge;
pub mod quilt;
pub mod optifine;

pub use fabric::*;
pub use forge::*;
pub use neoforge::*;
pub use quilt::*;
pub use optifine::*;

use anyhow::Result;

/// Trait for mod loader installers
pub trait LoaderInstaller {
    /// Install the mod loader for a specific Minecraft version
    fn install(&self, mc_version: &str, target_dir: &str) -> Result<String>;

    /// Get the loader version
    fn version(&self) -> &str;

    /// Get the loader type name
    fn loader_type(&self) -> &str;
}
