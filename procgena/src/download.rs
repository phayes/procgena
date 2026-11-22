//! Downloading published texture files and keeping verified copies on disk.
//!
//! Every file is checked against a SHA-256 checksum from a published list
//! before it is used or saved, so a damaged or outdated copy is never used.

use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use sha2::{Digest, Sha256};

/// The browser identification header included in download requests.
const USER_AGENT: &str = "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0 Safari/537.36";
/// The maximum download size for a single file, in bytes.
const MAX_BYTES: u64 = 64 * 1024 * 1024;
/// The time limit for each download request.
const TIMEOUT: Duration = Duration::from_secs(60);

/// Fetches a file's contents, given its path relative to a download location.
pub type FetchFn = dyn Fn(&str) -> Result<Vec<u8>, String> + Send + Sync;

/// Returns a function that downloads `<base_url><path>` over HTTPS, blocking
/// until the whole file arrives. `base_url` must end in `/`. Files larger
/// than 64 MB, and requests taking over a minute, fail.
pub fn http_fetch(base_url: impl Into<String>) -> impl Fn(&str) -> Result<Vec<u8>, String> {
    let base_url = base_url.into();
    let agent = ureq::Agent::config_builder()
        .user_agent(USER_AGENT)
        .timeout_global(Some(TIMEOUT))
        .build()
        .new_agent();
    move |path: &str| -> Result<Vec<u8>, String> {
        agent
            .get(format!("{base_url}{path}"))
            .call()
            .and_then(|mut response| {
                response
                    .body_mut()
                    .with_config()
                    .limit(MAX_BYTES)
                    .read_to_vec()
            })
            .map_err(|error| error.to_string())
    }
}

/// Returns the file at `path` whose checksum is `sha256`: the copy at
/// `cached` if it matches, or else a fresh download from `fetch`, which is
/// then saved to `cached`. With `cached` of `None`, always downloads.
///
/// Fails if the download fails or doesn't match the checksum; nothing is
/// saved then. A failure to save a good download is ignored. Blocks while
/// downloading.
pub fn cached_or_fetch(
    cached: Option<&Path>,
    path: &str,
    sha256: &str,
    fetch: &FetchFn,
) -> Result<Vec<u8>, String> {
    if let Some(cached) = cached
        && let Ok(bytes) = fs::read(cached)
        && hex_sha256(&bytes) == sha256
    {
        return Ok(bytes);
    }

    let bytes = fetch(path)?;
    if hex_sha256(&bytes) != sha256 {
        return Err("the download was damaged (its checksum does not match)".to_owned());
    }
    if let Some(cached) = cached {
        let _ = write_atomically(cached, &bytes);
    }
    Ok(bytes)
}

/// Computes a SHA-256 checksum in lowercase hexadecimal, the format the
/// published lists use.
pub fn hex_sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// Writes `bytes` to `path`, creating its folder, so that readers never see
/// a half-written file. Writes running at the same time don't interfere.
pub fn write_atomically(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    static WRITES: AtomicU64 = AtomicU64::new(0);
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let write = WRITES.fetch_add(1, Ordering::Relaxed);
    let mut partial = path.as_os_str().to_owned();
    partial.push(format!(".{}-{write}.part", std::process::id()));
    fs::write(&partial, bytes)?;
    fs::rename(&partial, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checksums_are_lowercase_hex_sha256() {
        assert_eq!(
            hex_sha256(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
}
