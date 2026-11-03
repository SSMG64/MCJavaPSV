//! Everything the builder does before it converts any game data: finding the
//! player's own Minecraft install, checking the files and Java it needs, and
//! running Horizon's `horizon-prepare` on them.

pub mod error;
pub mod java;
pub mod minecraft;
pub mod prepare;
pub mod process;
pub mod report;

pub use error::SetupError;
pub use minecraft::TARGET_VERSION;
