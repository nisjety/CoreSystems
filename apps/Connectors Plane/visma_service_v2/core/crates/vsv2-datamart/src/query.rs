use std::path::PathBuf;
use vsv2_common::{Error, Result};

/// On-disk `.graphql` file. Loaded lazily, cached after first read.
#[derive(Debug, Clone)]
pub struct QueryFile {
    pub path: PathBuf,
}

impl QueryFile {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn read(&self) -> Result<String> {
        std::fs::read_to_string(&self.path).map_err(Error::Io)
    }
}
