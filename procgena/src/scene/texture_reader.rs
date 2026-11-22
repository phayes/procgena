//! An asset source for a generator's texture files that downloads the ones
//! missing from the workspace.
//!
//! Each generator publishes its textures with a list of every file and its
//! SHA-256 checksum (`<generator>/assets/textures/assets_manifest.json`).
//! [`TextureReader`] reads a file from the workspace's assets folder when it
//! is there. Otherwise, if the list names it, it uses a verified copy in the
//! user's cache folder, downloading and saving one first if needed.

use std::collections::HashMap;
use std::io;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use bevy::asset::io::file::FileAssetReader;
use bevy::asset::io::{AssetReader, AssetReaderError, PathStream, Reader, VecReader};
use bevy::prelude::*;

use crate::download::{FetchFn, cached_or_fetch, http_fetch};

/// The folder under a generator's assets folder that holds its textures, and
/// that the published file list's paths are relative to.
const TEXTURES: &str = "textures";

/// A generator's published texture files, and where verified copies are kept.
pub struct TextureFiles {
    /// SHA-256 checksum of each published file, by its path under `textures/`.
    listed: HashMap<String, String>,
    /// Folder for downloaded copies, or `None` to keep them only in memory.
    cache: Option<PathBuf>,
    /// Downloads a published file, given its path under `textures/`.
    fetch: Box<FetchFn>,
}

impl TextureFiles {
    /// Reads the published file list `manifest` (the text of an
    /// `assets_manifest.json`) and prepares to download from its `baseUrl`,
    /// keeping copies in `cache`. Fails if the list is malformed.
    pub fn from_manifest(manifest: &str, cache: Option<PathBuf>) -> Result<Self, String> {
        let manifest: serde_json::Value = serde_json::from_str(manifest)
            .map_err(|error| format!("the texture list is damaged: {error}"))?;
        let base_url = manifest["baseUrl"]
            .as_str()
            .ok_or("the texture list has no baseUrl")?
            .to_owned();
        let assets = manifest["assets"]
            .as_array()
            .ok_or("the texture list has no assets")?;
        let listed = assets
            .iter()
            .filter_map(|asset| asset["files"].as_array())
            .flatten()
            .filter_map(|file| Some((file["path"].as_str()?, file["sha256"].as_str()?)))
            .map(|(path, sha256)| (path.to_owned(), sha256.to_owned()))
            .collect();
        Ok(Self::with_fetch(listed, cache, http_fetch(base_url)))
    }

    /// Publishes no files, so nothing is ever downloaded.
    pub fn none() -> Self {
        Self::with_fetch(HashMap::new(), None, |_| {
            Err("nothing is published".to_owned())
        })
    }

    /// Whether some published file's asset path starts with `prefix`, such
    /// as `textures/bark/`.
    #[cfg(test)]
    pub fn publishes_under(&self, prefix: &str) -> bool {
        self.listed
            .keys()
            .any(|path| format!("{TEXTURES}/{path}").starts_with(prefix))
    }

    /// Creates the files from their checksums by path, a cache folder and a
    /// download function.
    fn with_fetch(
        listed: HashMap<String, String>,
        cache: Option<PathBuf>,
        fetch: impl Fn(&str) -> Result<Vec<u8>, String> + Send + Sync + 'static,
    ) -> Self {
        Self {
            listed,
            cache,
            fetch: Box::new(fetch),
        }
    }

    /// The published path for `asset_path`, a path within the generator's
    /// assets folder such as `textures/cap/x.ktx2`, or `None` if the list
    /// doesn't name it.
    fn published(&self, asset_path: &Path) -> Option<String> {
        let rest = asset_path.strip_prefix(TEXTURES).ok()?;
        let parts: Option<Vec<&str>> = rest
            .components()
            .map(|part| match part {
                Component::Normal(name) => name.to_str(),
                _ => None,
            })
            .collect();
        let path = parts?.join("/");
        self.listed.contains_key(&path).then_some(path)
    }

    /// The contents of the published file `path`, from the cache or else
    /// downloaded, checked and saved to the cache. Blocks while downloading.
    fn get(&self, path: &str) -> Result<Vec<u8>, String> {
        let sha256 = &self.listed[path];
        let cached = self.cache.as_ref().map(|cache| cache.join(path));
        cached_or_fetch(cached.as_deref(), path, sha256, &*self.fetch)
    }
}

/// Reads a generator's assets folder, filling in published texture files
/// that aren't there from the cache or by downloading them.
///
/// Downloads run on their own threads, so other assets keep loading. A file
/// that can't be downloaded, or arrives damaged, fails to load.
pub struct TextureReader {
    /// The workspace's assets folder for the generator.
    local: FileAssetReader,
    files: Arc<TextureFiles>,
}

impl TextureReader {
    /// Creates a reader for the assets folder `folder` (an absolute path)
    /// whose missing textures come from `files`.
    pub fn new(folder: &Path, files: Arc<TextureFiles>) -> Self {
        Self {
            local: FileAssetReader::new(folder),
            files,
        }
    }
}

impl AssetReader for TextureReader {
    async fn read<'a>(&'a self, path: &'a Path) -> Result<Box<dyn Reader + 'a>, AssetReaderError> {
        // Use the workspace's copy when it has one.
        match self.local.read(path).await {
            Ok(reader) => return Ok(Box::new(reader)),
            Err(AssetReaderError::NotFound(_)) => {}
            Err(error) => return Err(error),
        }

        // Otherwise get the published file, if there is one.
        let Some(published) = self.files.published(path) else {
            return Err(AssetReaderError::NotFound(path.to_owned()));
        };
        let files = self.files.clone();
        let bytes = blocking::unblock(move || files.get(&published))
            .await
            .map_err(|error| {
                let message = format!("cannot get texture {}: {error}", path.display());
                AssetReaderError::Io(Arc::new(io::Error::other(message)))
            })?;
        Ok(Box::new(VecReader::new(bytes)))
    }

    async fn read_meta<'a>(
        &'a self,
        path: &'a Path,
    ) -> Result<Box<dyn Reader + 'a>, AssetReaderError> {
        // Settings files are never published, so they are never downloaded.
        let reader = self.local.read_meta(path).await?;
        Ok(Box::new(reader))
    }

    async fn read_directory<'a>(
        &'a self,
        path: &'a Path,
    ) -> Result<Box<PathStream>, AssetReaderError> {
        self.local.read_directory(path).await
    }

    async fn is_directory<'a>(&'a self, path: &'a Path) -> Result<bool, AssetReaderError> {
        self.local.is_directory(path).await
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use bevy::tasks::block_on;

    use super::*;
    use crate::download::hex_sha256;

    const CAP: &str = "cap/a-1k-color-bc7.ktx2";

    /// An empty temporary folder for this test.
    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir()
            .join("procgena-tests")
            .join(format!("textures-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A download source serving `served`, counting requests.
    #[derive(Clone, Default)]
    struct Server {
        served: Arc<Mutex<HashMap<String, Vec<u8>>>>,
        requests: Arc<AtomicUsize>,
    }

    impl Server {
        fn serve(&self, path: &str, bytes: &[u8]) {
            self.served
                .lock()
                .unwrap()
                .insert(path.to_owned(), bytes.to_vec());
        }

        fn requests(&self) -> usize {
            self.requests.load(Ordering::SeqCst)
        }

        /// A reader for the assets folder `folder`, caching in `cache`, for a
        /// list publishing `CAP` as `published`.
        fn reader(&self, folder: &Path, cache: &Path, published: &[u8]) -> TextureReader {
            let server = self.clone();
            let listed = HashMap::from([(CAP.to_owned(), hex_sha256(published))]);
            let files = TextureFiles::with_fetch(listed, Some(cache.to_owned()), move |path| {
                server.requests.fetch_add(1, Ordering::SeqCst);
                let served = server.served.lock().unwrap();
                served
                    .get(path)
                    .cloned()
                    .ok_or_else(|| "offline".to_owned())
            });
            TextureReader::new(folder, Arc::new(files))
        }
    }

    /// Reads `path` through `reader`.
    fn read(reader: &TextureReader, path: &str) -> Result<Vec<u8>, AssetReaderError> {
        block_on(async {
            let mut file = reader.read(Path::new(path)).await?;
            let mut bytes = Vec::new();
            file.read_to_end(&mut bytes).await.unwrap();
            Ok(bytes)
        })
    }

    #[test]
    fn a_workspace_file_is_read_without_downloading() {
        let (folder, cache) = (temp("local"), temp("local-cache"));
        fs::create_dir_all(folder.join("textures/cap")).unwrap();
        fs::write(folder.join("textures").join(CAP), b"local").unwrap();
        let server = Server::default();
        let reader = server.reader(&folder, &cache, b"published");
        assert_eq!(read(&reader, &format!("textures/{CAP}")).unwrap(), b"local");
        assert_eq!(server.requests(), 0);
    }

    #[test]
    fn a_missing_file_is_downloaded_once_then_read_from_the_cache() {
        let (folder, cache) = (temp("download"), temp("download-cache"));
        let server = Server::default();
        server.serve(CAP, b"cap");
        let reader = server.reader(&folder, &cache, b"cap");
        let path = format!("textures/{CAP}");
        assert_eq!(read(&reader, &path).unwrap(), b"cap");
        assert_eq!(read(&reader, &path).unwrap(), b"cap");
        assert_eq!(server.requests(), 1);
        assert_eq!(fs::read(cache.join(CAP)).unwrap(), b"cap");
        assert!(!folder.join(&path).exists(), "the workspace is left alone");
    }

    #[test]
    fn a_damaged_download_fails_and_is_not_cached() {
        let (folder, cache) = (temp("damaged"), temp("damaged-cache"));
        let server = Server::default();
        server.serve(CAP, b"bad");
        let reader = server.reader(&folder, &cache, b"cap");
        let error = read(&reader, &format!("textures/{CAP}")).unwrap_err();
        assert!(error.to_string().contains("checksum"), "{error}");
        assert!(!cache.join(CAP).exists());
    }

    #[test]
    fn a_stale_cached_file_is_replaced() {
        let (folder, cache) = (temp("stale"), temp("stale-cache"));
        fs::create_dir_all(cache.join("cap")).unwrap();
        fs::write(cache.join(CAP), b"old").unwrap();
        let server = Server::default();
        server.serve(CAP, b"new");
        let reader = server.reader(&folder, &cache, b"new");
        assert_eq!(read(&reader, &format!("textures/{CAP}")).unwrap(), b"new");
        assert_eq!(fs::read(cache.join(CAP)).unwrap(), b"new");
    }

    #[test]
    fn unpublished_files_and_settings_files_are_never_downloaded() {
        let (folder, cache) = (temp("unlisted"), temp("unlisted-cache"));
        let server = Server::default();
        let reader = server.reader(&folder, &cache, b"cap");
        for path in [
            "textures/cap/other.ktx2",
            "cap/a-1k-color-bc7.ktx2",
            "textures/../x",
        ] {
            assert!(matches!(
                read(&reader, path),
                Err(AssetReaderError::NotFound(_))
            ));
        }
        let path = PathBuf::from(format!("textures/{CAP}"));
        let meta = block_on(reader.read_meta(&path)).map(|_| ());
        assert!(matches!(meta, Err(AssetReaderError::NotFound(_))));
        assert_eq!(server.requests(), 0);
    }
}
