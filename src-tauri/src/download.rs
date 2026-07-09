//! Real HTTP `Downloader` (ticket 7's 接缝 2 adapter).
//!
//! Streams a model file over HTTPS to disk, relaying byte progress. The
//! orchestration (integrity check, retry mapping) lives in `models.rs` and is
//! tested with a fake; this adapter is covered by manual runs.

use crate::models::Downloader;
use std::fs::File;
use std::io::{Read, Write};
use std::path::Path;

/// `Downloader` backed by a blocking reqwest client streaming to disk.
pub struct HttpDownloader;

impl Downloader for HttpDownloader {
    fn download(
        &self,
        url: &str,
        dest: &Path,
        on_progress: &mut dyn FnMut(u64, Option<u64>),
    ) -> Result<(), String> {
        let mut resp = reqwest::blocking::get(url).map_err(|e| e.to_string())?;
        if !resp.status().is_success() {
            return Err(format!("HTTP {}", resp.status()));
        }
        let total = resp.content_length();

        // Stream to a `.part` file so an interrupted download never looks
        // complete; rename into place only after the bytes are all written.
        let part = dest.with_extension("part");
        let mut file = File::create(&part).map_err(|e| e.to_string())?;
        let mut downloaded = 0u64;
        let mut buf = [0u8; 64 * 1024];
        loop {
            let n = resp.read(&mut buf).map_err(|e| e.to_string())?;
            if n == 0 {
                break;
            }
            file.write_all(&buf[..n]).map_err(|e| e.to_string())?;
            downloaded += n as u64;
            on_progress(downloaded, total);
        }
        file.flush().map_err(|e| e.to_string())?;
        drop(file);
        std::fs::rename(&part, dest).map_err(|e| e.to_string())?;
        Ok(())
    }
}
