// Game control module (Alpha 6)
//
// Gives the launcher (and the agent API) the ability to observe and operate
// a running Minecraft instance without stealing the user's focus, keyboard,
// or mouse:
//
// - `window`   : locate the game window by instance name / PID (Windows)
// - `capture`  : high-performance screenshots via Windows.Graphics.Capture
//                (works while the window is unfocused or occluded)
// - `client`   : TCP protocol client for the MDL companion mod, which
//                injects input inside the game process
// - `install`  : install the bundled companion mod into an instance
// - `options`  : options.txt management (e.g. pauseOnLostFocus:false so the
//                game keeps running while the user focuses other apps)

pub mod client;
pub mod install;
pub mod options;

#[cfg(windows)]
pub mod capture;

#[cfg(windows)]
pub mod window;

/// Protocol version spoken between MDL and the companion mod.
pub const PROTOCOL_VERSION: u32 = 1;

/// Default TCP port the companion mod listens on. The launcher passes the
/// actual port to the game via the `mdl.agent.port` system property; the
/// companion writes the bound port to `runtime/agent.port` for discovery.
pub const DEFAULT_COMPANION_PORT: u16 = 25590;

/// JVM system property used to hand the companion port to the game process.
pub const COMPANION_PORT_PROPERTY: &str = "mdl.agent.port";

/// Filename the companion mod writes into the instance runtime directory
/// once its control server is accepting connections.
pub const COMPANION_PORT_FILE: &str = "agent.port";

/// Prefix of the companion mod JAR filename shipped alongside mdl.exe.
pub const COMPANION_JAR_PREFIX: &str = "mdl-agent-companion";
