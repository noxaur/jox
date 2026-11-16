#[cfg(target_os = "linux")]
pub mod mpris;
#[cfg(not(target_os = "linux"))]
#[path = "mpris_stub.rs"]
pub mod mpris;
pub mod player;
