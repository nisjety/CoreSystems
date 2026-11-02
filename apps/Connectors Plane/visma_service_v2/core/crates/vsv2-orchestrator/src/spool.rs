//! The records of one run on their way to the warehouse: one JSON line per record, in a temporary
//! file.
//!
//! A run writes each record here as its page arrives, so memory holds one page however large the
//! endpoint is. The warehouse loader reads the file in chunks. The file is removed when the run
//! ends, whether or not the records reached the warehouse.

use std::fs::File;
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::Path;

use serde_json::Value;
use tempfile::NamedTempFile;
use vsv2_common::{Error, Result};

pub(crate) struct Spool {
    file: BufWriter<NamedTempFile>,
    rows: u64,
}

impl Spool {
    pub(crate) fn new() -> Result<Self> {
        let file = NamedTempFile::new()
            .map_err(|e| Error::Other(format!("spool: create temporary file: {e}")))?;
        Ok(Self {
            file: BufWriter::new(file),
            rows: 0,
        })
    }

    /// Append one record, as a line of JSON.
    pub(crate) fn push(&mut self, row: &Value) -> Result<()> {
        serde_json::to_writer(&mut self.file, row)?;
        self.file.write_all(b"\n").map_err(Error::Io)?;
        self.rows += 1;
        Ok(())
    }

    pub(crate) fn rows(&self) -> u64 {
        self.rows
    }

    /// The file's path, with everything written so far on disk: the loader reads it by this path.
    pub(crate) fn path(&mut self) -> Result<&Path> {
        self.file.flush().map_err(Error::Io)?;
        Ok(self.file.get_ref().path())
    }

    /// Every record, read back. Only for the paths that take a list of records.
    pub(crate) fn read_all(&mut self) -> Result<Vec<Value>> {
        let path = self.path()?.to_path_buf();
        read_rows(&path)
    }
}

/// The records of a spool file, read back one line at a time.
pub(crate) fn read_rows(path: &Path) -> Result<Vec<Value>> {
    let file = File::open(path).map_err(Error::Io)?;
    let mut rows = Vec::new();
    for line in BufReader::new(file).lines() {
        let line = line.map_err(Error::Io)?;
        if !line.is_empty() {
            rows.push(serde_json::from_str(&line)?);
        }
    }
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn records_come_back_in_the_order_they_were_pushed() {
        let mut spool = Spool::new().expect("spool");

        spool.push(&json!({"a": 1})).expect("push");
        spool
            .push(&json!({"a": 2, "text": "æøå \"quoted\""}))
            .expect("push");

        assert_eq!(spool.rows(), 2);
        assert_eq!(
            spool.read_all().expect("read"),
            vec![json!({"a": 1}), json!({"a": 2, "text": "æøå \"quoted\""})]
        );
    }

    #[test]
    fn the_loader_reads_the_same_records_by_the_path() {
        let mut spool = Spool::new().expect("spool");
        spool.push(&json!({"a": 1})).expect("push");
        let path = spool.path().expect("path").to_path_buf();

        assert_eq!(read_rows(&path).expect("read"), vec![json!({"a": 1})]);
    }

    #[test]
    fn the_file_is_removed_when_the_spool_ends() {
        let mut spool = Spool::new().expect("spool");
        let path = spool.path().expect("path").to_path_buf();
        assert!(path.exists());

        drop(spool);

        assert!(!path.exists());
    }
}
