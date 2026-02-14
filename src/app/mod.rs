//! Application Module
//!
//! Manages application lifecycle and state.

use anyhow::Result;

pub struct App {
    pub running: bool,
}

impl App {
    pub fn new() -> Result<Self> {
        Ok(Self { running: true })
    }

    pub async fn run(&mut self) -> Result<()> {
        tracing::info!("Application starting...");
        Ok(())
    }
}

impl Default for App {
    fn default() -> Self {
        Self::new().expect("Failed to create App")
    }
}
