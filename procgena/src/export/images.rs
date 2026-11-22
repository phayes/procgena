//! Loads bark, leaf and rock texture images for exported models.
//!
//! Downloads come from the public `ze_textures` collection. Local cached copies
//! can be reused, including offline when the image list is also cached.
//! [`fetch_images`] returns available images and warnings about missing textures.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use sha2::{Digest, Sha256};

/// Base URL for tree bark and leaf textures and their published image list.
pub const TREE_IMAGES: &str = "https://storage.googleapis.com/ze_textures/tree/images/";
/// Base URL for rock textures and their published image list.
pub const STONE_IMAGES: &str = "https://storage.googleapis.com/ze_textures/stone/images/";
/// The file list containing image paths and checksums, used to verify downloads.
const MANIFEST: &str = "images_manifest.json";
/// The browser identification header included in download requests.
const USER_AGENT: &str = "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0 Safari/537.36";
/// The maximum download size for a single file, in bytes.
const MAX_BYTES: u64 = 64 * 1024 * 1024;
/// The time limit for each download request.
const TIMEOUT: Duration = Duration::from_secs(60);

/// Fetches file contents using a path relative to the texture download location.
type FetchFn = dyn Fn(&str) -> Result<Vec<u8>, String> + Send + Sync;

/// Available texture images and explanations for any that could not be loaded.
#[derive(Debug, Default)]
pub struct Images {
    /// Image contents indexed by relative path, such as `"bark/oak_bark-1k-color.jpg"`.
    pub found: HashMap<String, Vec<u8>>,
    /// Messages explaining missing images or an unavailable image list.
    pub problems: Vec<String>,
}

/// Loads texture images for export and keeps local copies for later use.
/// Checksums from the published file list detect damaged or outdated copies.
pub struct ImageStore {
    cache: PathBuf,
    fetch: Box<FetchFn>,
}

impl ImageStore {
    /// Creates an image store for `base_url`, such as [`TREE_IMAGES`] or [`STONE_IMAGES`].
    /// The URL must end in `/` and provide `images_manifest.json`. Use a distinct
    /// `cache_name` folder name for each source to keep cached images separate.
    /// Downloads begin when images are requested. Returns `None` if the system
    /// has no cache folder.
    pub fn new(base_url: &'static str, cache_name: &str) -> Option<Self> {
        let cache = dirs::cache_dir()?
            .join("procgena")
            .join("export_images")
            .join(cache_name);
        let agent = ureq::Agent::config_builder()
            .user_agent(USER_AGENT)
            .timeout_global(Some(TIMEOUT))
            .build()
            .new_agent();
        let fetch = move |path: &str| -> Result<Vec<u8>, String> {
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
        };
        Some(Self::with_fetch(cache, fetch))
    }

    /// Creates a store with a chosen cache location and file-fetching function.
    fn with_fetch(
        cache: PathBuf,
        fetch: impl Fn(&str) -> Result<Vec<u8>, String> + Send + Sync + 'static,
    ) -> Self {
        Self {
            cache,
            fetch: Box::new(fetch),
        }
    }

    /// Loads the requested images from the cache or downloads them as needed.
    /// Returns available images alongside any problems; an unavailable file list
    /// produces a single shared problem. Waits for downloads to finish before returning.
    pub fn images(&self, paths: &[String]) -> Images {
        let mut images = Images::default();
        if paths.is_empty() {
            return images;
        }
        let listed = match self.manifest() {
            Ok(listed) => listed,
            Err(why) => {
                images
                    .problems
                    .push(format!("Could not get the list of texture images: {why}"));
                return images;
            }
        };

        for path in paths {
            let result = match listed.get(path) {
                Some(sha256) => self.image(path, sha256),
                None => Err("it is not a published texture".to_owned()),
            };
            match result {
                Ok(bytes) => {
                    images.found.insert(path.clone(), bytes);
                }
                Err(why) => images.problems.push(format!("No image {path}: {why}")),
            }
        }
        images
    }

    /// Loads the published image paths and SHA-256 checksums. Uses the cached list
    /// if a fresh list cannot be downloaded or read.
    fn manifest(&self) -> Result<HashMap<String, String>, String> {
        let cached = self.cache.join(MANIFEST);
        match (self.fetch)(MANIFEST).and_then(|bytes| parse_manifest(&bytes).map(|m| (m, bytes))) {
            Ok((listed, bytes)) => {
                // The image list remains usable even if saving a local copy fails.
                let _ = write_atomically(&cached, &bytes);
                Ok(listed)
            }
            Err(download) => {
                let bytes = fs::read(&cached).map_err(|_| download.clone())?;
                parse_manifest(&bytes).map_err(|_| download)
            }
        }
    }

    /// Reuses a cached image if its checksum matches the published version.
    /// Otherwise downloads and verifies a replacement before trying to cache it.
    fn image(&self, path: &str, sha256: &str) -> Result<Vec<u8>, String> {
        let cached = self.cache.join(path);
        if let Ok(bytes) = fs::read(&cached)
            && hex_sha256(&bytes) == sha256
        {
            return Ok(bytes);
        }

        let bytes = (self.fetch)(path)?;
        if hex_sha256(&bytes) != sha256 {
            return Err("the download was damaged (its checksum does not match)".to_owned());
        }
        // The downloaded image remains usable even if saving a local copy fails.
        let _ = write_atomically(&cached, &bytes);
        Ok(bytes)
    }
}

/// Returns available image bytes keyed by relative path, plus warnings for
/// missing images. Waits for any downloads to finish. If `store` is `None`,
/// returns no images and a warning, unless `paths` is empty. Callers can still
/// export using material colours when textures are unavailable.
pub fn fetch_images(
    store: Option<&ImageStore>,
    paths: &[String],
) -> (HashMap<String, Vec<u8>>, Vec<String>) {
    if paths.is_empty() {
        return Default::default();
    }
    let Some(store) = store else {
        return (
            Default::default(),
            vec!["Exported colours only: this system has no cache folder for textures.".to_owned()],
        );
    };
    let images = store.images(paths);
    let mut warnings = images.problems;
    if !warnings.is_empty() {
        warnings.insert(
            0,
            "Some textures are missing, so their colours are used:".to_owned(),
        );
    }
    (images.found, warnings)
}

/// Reads image paths and their verification checksums from the published JSON list.
fn parse_manifest(bytes: &[u8]) -> Result<HashMap<String, String>, String> {
    let manifest: serde_json::Value =
        serde_json::from_slice(bytes).map_err(|error| format!("the list is damaged: {error}"))?;
    let assets = manifest["assets"]
        .as_array()
        .ok_or("the list has no assets")?;
    let mut listed = HashMap::new();
    for file in assets
        .iter()
        .filter_map(|asset| asset["files"].as_array())
        .flatten()
    {
        if let (Some(path), Some(sha256)) = (file["path"].as_str(), file["sha256"].as_str()) {
            listed.insert(path.to_owned(), sha256.to_owned());
        }
    }
    Ok(listed)
}

/// Computes a SHA-256 checksum in the lowercase hexadecimal format used by the list.
fn hex_sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// Writes to a temporary file before renaming it to the destination, so readers
/// do not see a file while it is still being written. Each write uses its own
/// temporary file, so exports running at the same time do not interfere.
fn write_atomically(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
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
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    use super::*;

    /// Provides an empty temporary cache folder for this test.
    fn cache_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir()
            .join("procgena-tests")
            .join(format!("{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    /// Builds a test image list with checksums calculated from the supplied contents.
    fn manifest(files: &[(&str, &[u8])]) -> Vec<u8> {
        let files: Vec<_> = files
            .iter()
            .map(|(path, bytes)| serde_json::json!({"path": path, "sha256": hex_sha256(bytes)}))
            .collect();
        serde_json::to_vec(&serde_json::json!({"assets": [{"id": "x", "files": files}]})).unwrap()
    }

    /// A test download source that stores files and counts requests.
    #[derive(Clone, Default)]
    struct Server {
        files: Arc<Mutex<HashMap<String, Vec<u8>>>>,
        requests: Arc<AtomicUsize>,
    }

    impl Server {
        fn serve(&self, path: &str, bytes: &[u8]) {
            self.files
                .lock()
                .unwrap()
                .insert(path.to_owned(), bytes.to_vec());
        }

        fn store(&self, cache: PathBuf) -> ImageStore {
            let server = self.clone();
            ImageStore::with_fetch(cache, move |path| {
                server.requests.fetch_add(1, Ordering::SeqCst);
                server
                    .files
                    .lock()
                    .unwrap()
                    .get(path)
                    .cloned()
                    .ok_or_else(|| "offline".to_owned())
            })
        }

        fn requests(&self) -> usize {
            self.requests.load(Ordering::SeqCst)
        }
    }

    const BARK: &str = "bark/a-1k-color.jpg";

    #[test]
    fn images_are_downloaded_once_then_read_from_the_cache() {
        let server = Server::default();
        server.serve(MANIFEST, &manifest(&[(BARK, b"bark")]));
        server.serve(BARK, b"bark");
        let store = server.store(cache_dir("cached"));

        let first = store.images(&[BARK.to_owned()]);
        assert_eq!(first.found[BARK], b"bark");
        assert!(first.problems.is_empty());
        assert_eq!(server.requests(), 2, "the manifest and the image");

        // The second export checks the file list again but reuses the cached image.
        let second = store.images(&[BARK.to_owned()]);
        assert_eq!(second.found[BARK], b"bark");
        assert_eq!(server.requests(), 3);
    }

    #[test]
    fn a_damaged_download_is_a_problem_and_is_not_cached() {
        let server = Server::default();
        server.serve(MANIFEST, &manifest(&[(BARK, b"bark")]));
        server.serve(BARK, b"bad");
        let cache = cache_dir("damaged");
        let images = server.store(cache.clone()).images(&[BARK.to_owned()]);
        assert!(images.found.is_empty());
        assert_eq!(images.problems.len(), 1);
        assert!(
            images.problems[0].contains("checksum"),
            "{:?}",
            images.problems
        );
        assert!(!cache.join(BARK).exists());
    }

    #[test]
    fn a_changed_image_replaces_the_cached_one() {
        let server = Server::default();
        server.serve(MANIFEST, &manifest(&[(BARK, b"old")]));
        server.serve(BARK, b"old");
        let store = server.store(cache_dir("changed"));
        store.images(&[BARK.to_owned()]);

        // Publish replacement image contents and a matching checksum.
        server.serve(MANIFEST, &manifest(&[(BARK, b"new")]));
        server.serve(BARK, b"new");
        assert_eq!(store.images(&[BARK.to_owned()]).found[BARK], b"new");
    }

    #[test]
    fn offline_exports_use_the_cache() {
        let server = Server::default();
        server.serve(MANIFEST, &manifest(&[(BARK, b"bark")]));
        server.serve(BARK, b"bark");
        let cache = cache_dir("offline");
        server.store(cache.clone()).images(&[BARK.to_owned()]);

        let offline = Server::default().store(cache);
        let images = offline.images(&[BARK.to_owned()]);
        assert_eq!(images.found[BARK], b"bark");
        assert!(images.problems.is_empty());
    }

    #[test]
    fn missing_images_are_problems() {
        // Without a connection or cached list, one problem explains all missing images.
        let images = Server::default()
            .store(cache_dir("empty"))
            .images(&[BARK.to_owned(), "leaves/a.png".to_owned()]);
        assert!(images.found.is_empty());
        assert_eq!(images.problems.len(), 1, "{:?}", images.problems);

        // Reject an image path that is absent from the published list.
        let server = Server::default();
        server.serve(MANIFEST, &manifest(&[(BARK, b"bark")]));
        let images = server
            .store(cache_dir("unlisted"))
            .images(&["bark/b-1k-color.jpg".to_owned()]);
        assert!(images.problems[0].contains("not a published texture"));
    }

    #[test]
    fn checksums_are_lowercase_hex_sha256() {
        assert_eq!(
            hex_sha256(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
}
