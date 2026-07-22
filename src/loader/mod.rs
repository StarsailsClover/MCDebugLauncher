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

