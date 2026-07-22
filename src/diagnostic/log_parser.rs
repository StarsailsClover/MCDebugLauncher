// Log parser

use anyhow::Result;

pub struct LogParser {
}

impl LogParser {
    pub fn new() -> Self {
        Self {}
    }

    pub fn parse(&self, _log_path: &str) -> Result<Vec<String>> {
        // TODO: Implement log parsing
        Ok(vec![])
    }
}
