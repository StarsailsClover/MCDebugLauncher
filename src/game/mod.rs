// Game control module (Alpha 6)
//
// Gives the launcher (and the agent API) the ability to observe and operate
// a running Minecraft instance without stealing the user's focus, keyboard,
// or mouse:
//
// - `window`   : locate the game window by instance name / PID (Windows)
// - `capture`  : high-performance screenshots via Windows.Graphics.Capture
//                (works while the window is unfocused or occluded)
// - `client`   : HTTP client for the Despotes control mod, which injects
//                input inside the game process
// - `despotes` : detect, download and install the Despotes control mod
// - `options`  : options.txt management (e.g. pauseOnLostFocus:false so the
//                game keeps running while the user focuses other apps)

pub mod client;
pub mod despotes;
pub mod options;
pub mod watchdog;

#[cfg(windows)]
pub mod capture;

#[cfg(windows)]
pub mod window;

/// Default HTTP port of the Despotes in-game control server. The launcher
/// passes the port to the game via the `despotes.port` system property and
/// records it in `runtime/despotes.port` for post-launch discovery.
pub const DEFAULT_DESPOTES_PORT: u16 = 25585;

/// JVM system property Despotes honours to override its HTTP port.
pub const DESPOTES_PORT_PROPERTY: &str = "despotes.port";

/// File the launcher writes the requested Despotes port into.
pub const DESPOTES_PORT_FILE: &str = "despotes.port";
