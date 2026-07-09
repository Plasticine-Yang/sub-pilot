//! Whisper model registry and on-demand download orchestration (ticket 7).
//!
//! The registry (name → filename, URL, SHA-256) and availability check are
//! pure logic (接缝 1-adjacent). Download orchestration depends on a
//! `Downloader` trait (接缝 2) so it is exercised with a fake; the real HTTP
//! download lives in the adapter and is covered by manual runs.

use std::path::{Path, PathBuf};

/// A selectable Whisper model. `base` is bundled (ADR-0002); the rest download
/// on demand. Serialized to the frontend for the model picker.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ModelId {
    Tiny,
    Base,
    Small,
    Medium,
    LargeV3,
}

/// Static metadata for a model: the on-disk filename, download URL, and the
/// expected SHA-256 used both to name the URL and to verify integrity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelInfo {
    pub id: ModelId,
    pub file_name: String,
    pub url: String,
    pub sha256: String,
    /// Whether this model ships inside the app bundle (no download needed).
    pub bundled: bool,
}

impl ModelId {
    /// The registry entry for this model. Official OpenAI Whisper `.pt` weights;
    /// each URL embeds the same SHA-256 we verify against (ADR-0005/0002).
    pub fn info(self) -> ModelInfo {
        let (file_name, url, sha256, bundled) = match self {
            ModelId::Tiny => (
                "tiny.pt",
                "https://openaipublic.azureedge.net/main/whisper/models/65147644a518d12f04e32d6f3b26facc3f8dd46e0f569b7488a6a2d4b74e7c0e/tiny.pt",
                "65147644a518d12f04e32d6f3b26facc3f8dd46e0f569b7488a6a2d4b74e7c0e",
                false,
            ),
            ModelId::Base => (
                "base.pt",
                "https://openaipublic.azureedge.net/main/whisper/models/ed3a0b6b1c0edf879ad9b11b1af5a0e6ab5db9205f891f668f8b0e6c6326e34e/base.pt",
                "ed3a0b6b1c0edf879ad9b11b1af5a0e6ab5db9205f891f668f8b0e6c6326e34e",
                true,
            ),
            ModelId::Small => (
                "small.pt",
                "https://openaipublic.azureedge.net/main/whisper/models/9ecf779972d90ba49c06d968637d720dd632c55bbf19d441fb42bf17a411e794/small.pt",
                "9ecf779972d90ba49c06d968637d720dd632c55bbf19d441fb42bf17a411e794",
                false,
            ),
            ModelId::Medium => (
                "medium.pt",
                "https://openaipublic.azureedge.net/main/whisper/models/345ae4da62f9b3d59415adc60127b97c714f32e89e936602e85993674d08dcb1/medium.pt",
                "345ae4da62f9b3d59415adc60127b97c714f32e89e936602e85993674d08dcb1",
                false,
            ),
            ModelId::LargeV3 => (
                "large-v3.pt",
                "https://openaipublic.azureedge.net/main/whisper/models/e5b1a55b89c1367dacf97e3e19bfd829a01529dbfdeefa8caeb59b3f1b81dadb/large-v3.pt",
                "e5b1a55b89c1367dacf97e3e19bfd829a01529dbfdeefa8caeb59b3f1b81dadb",
                false,
            ),
        };
        ModelInfo {
            id: self,
            file_name: file_name.to_string(),
            url: url.to_string(),
            sha256: sha256.to_string(),
            bundled,
        }
    }

    /// The model name whisper expects on its `--model` flag.
    pub fn as_str(self) -> &'static str {
        match self {
            ModelId::Tiny => "tiny",
            ModelId::Base => "base",
            ModelId::Small => "small",
            ModelId::Medium => "medium",
            ModelId::LargeV3 => "large-v3",
        }
    }

    /// All models in picker order (smallest → largest).
    pub fn all() -> [ModelId; 5] {
        [
            ModelId::Tiny,
            ModelId::Base,
            ModelId::Small,
            ModelId::Medium,
            ModelId::LargeV3,
        ]
    }
}

/// A model's presence on disk, for the picker.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelStatus {
    pub id: ModelId,
    pub name: String,
    pub bundled: bool,
    /// Whether the `<model>.pt` weight file exists in the model directory.
    pub downloaded: bool,
}

/// Reports each model's download status. A bundled model is looked up in
/// `bundled_dir` (the read-only app resources); the rest live in
/// `downloads_dir` (the writable app-data dir, ADR-0003). Pure given the two
/// directories — it does the cheap `is_file` probe.
pub fn model_statuses(bundled_dir: &Path, downloads_dir: &Path) -> Vec<ModelStatus> {
    ModelId::all()
        .into_iter()
        .map(|id| {
            let info = id.info();
            let dir = if info.bundled { bundled_dir } else { downloads_dir };
            ModelStatus {
                id,
                name: id.as_str().to_string(),
                bundled: info.bundled,
                downloaded: dir.join(&info.file_name).is_file(),
            }
        })
        .collect()
}

/// Fetches bytes for a URL, reporting downloaded/total bytes as it goes. The
/// real impl streams over HTTP; tests use a fake.
pub trait Downloader {
    /// Downloads `url` to `dest`, calling `on_progress(downloaded, total)` as
    /// bytes arrive (`total` is `None` if the server omits Content-Length).
    fn download(
        &self,
        url: &str,
        dest: &Path,
        on_progress: &mut dyn FnMut(u64, Option<u64>),
    ) -> Result<(), String>;
}

/// Why an on-demand model download failed.
#[derive(Debug, PartialEq, Eq)]
pub enum DownloadError {
    /// The model ships in the bundle; downloading it is a programming error.
    AlreadyBundled,
    /// The network/adapter download failed.
    Fetch(String),
    /// The downloaded file's SHA-256 did not match the registry.
    ChecksumMismatch { expected: String, got: String },
}

impl std::fmt::Display for DownloadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DownloadError::AlreadyBundled => write!(f, "该模型已随应用打包，无需下载"),
            DownloadError::Fetch(m) => write!(f, "下载失败：{m}（请检查网络后重试）"),
            DownloadError::ChecksumMismatch { .. } => {
                write!(f, "下载文件校验未通过，可能已损坏（请重试）")
            }
        }
    }
}

impl std::error::Error for DownloadError {}

/// Ensures `info`'s weights are present in `model_dir`, downloading them if
/// missing and verifying the SHA-256. Idempotent: a present file is a no-op. On
/// checksum failure the corrupt download is removed so a retry starts clean.
/// Progress (downloaded/total bytes) is relayed via `on_progress`.
pub fn ensure_model(
    downloader: &dyn Downloader,
    model_dir: &Path,
    info: &ModelInfo,
    on_progress: &mut dyn FnMut(u64, Option<u64>),
) -> Result<PathBuf, DownloadError> {
    let dest = model_dir.join(&info.file_name);
    if dest.is_file() {
        return Ok(dest);
    }
    if info.bundled {
        // A bundled model should already exist; if it doesn't, the caller has
        // the wrong directory — don't try to fetch it from the network.
        return Err(DownloadError::AlreadyBundled);
    }

    downloader
        .download(&info.url, &dest, on_progress)
        .map_err(DownloadError::Fetch)?;

    let got = sha256_file(&dest).map_err(DownloadError::Fetch)?;
    if got != info.sha256 {
        let _ = std::fs::remove_file(&dest);
        return Err(DownloadError::ChecksumMismatch {
            expected: info.sha256.to_string(),
            got,
        });
    }
    Ok(dest)
}

/// Computes the lowercase hex SHA-256 of a file's contents.
fn sha256_file(path: &Path) -> Result<String, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("读取下载文件失败：{e}"))?;
    Ok(sha256_hex(&bytes))
}

/// Pure SHA-256 over bytes, returned as lowercase hex. Small, dependency-free
/// implementation (FIPS 180-4) so integrity checks need no extra crates.
pub fn sha256_hex(data: &[u8]) -> String {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];
    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];

    let mut msg = data.to_vec();
    let bit_len = (data.len() as u64).wrapping_mul(8);
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bit_len.to_be_bytes());

    for chunk in msg.chunks_exact(64) {
        let mut w = [0u32; 64];
        for (i, word) in chunk.chunks_exact(4).enumerate() {
            w[i] = u32::from_be_bytes([word[0], word[1], word[2], word[3]]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }

        let mut a = h;
        for i in 0..64 {
            let s1 = a[4].rotate_right(6) ^ a[4].rotate_right(11) ^ a[4].rotate_right(25);
            let ch = (a[4] & a[5]) ^ ((!a[4]) & a[6]);
            let t1 = a[7]
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let s0 = a[0].rotate_right(2) ^ a[0].rotate_right(13) ^ a[0].rotate_right(22);
            let maj = (a[0] & a[1]) ^ (a[0] & a[2]) ^ (a[1] & a[2]);
            let t2 = s0.wrapping_add(maj);
            a = [
                t1.wrapping_add(t2),
                a[0],
                a[1],
                a[2],
                a[3].wrapping_add(t1),
                a[4],
                a[5],
                a[6],
            ];
        }
        for (i, v) in a.iter().enumerate() {
            h[i] = h[i].wrapping_add(*v);
        }
    }

    let mut out = String::with_capacity(64);
    for word in h {
        out.push_str(&format!("{word:08x}"));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    // --- sha256 (pure) against known vectors --------------------------------

    #[test]
    fn sha256_matches_known_vectors() {
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    // --- registry (pure) ----------------------------------------------------

    #[test]
    fn base_is_the_only_bundled_model() {
        for id in ModelId::all() {
            assert_eq!(id.info().bundled, id == ModelId::Base, "{id:?}");
        }
    }

    #[test]
    fn each_model_url_embeds_its_checksum() {
        for id in ModelId::all() {
            let info = id.info();
            assert!(
                info.url.contains(&info.sha256),
                "{id:?} url should embed its sha256"
            );
        }
    }

    #[test]
    fn statuses_reflect_files_on_disk() {
        let bundled = tempfile::tempdir().unwrap();
        let downloads = tempfile::tempdir().unwrap();
        std::fs::write(downloads.path().join("small.pt"), b"weights").unwrap();
        // Bundled base lives in the read-only resources dir, not downloads.
        std::fs::write(bundled.path().join("base.pt"), b"bundled").unwrap();

        let statuses = model_statuses(bundled.path(), downloads.path());
        let small = statuses.iter().find(|s| s.id == ModelId::Small).unwrap();
        let medium = statuses.iter().find(|s| s.id == ModelId::Medium).unwrap();
        let base = statuses.iter().find(|s| s.id == ModelId::Base).unwrap();
        assert!(small.downloaded);
        assert!(!medium.downloaded);
        assert!(base.downloaded, "bundled base found in bundled dir");
    }

    // --- download orchestration (接缝 2) ------------------------------------

    /// Writes fixed contents and replays a canned progress sequence.
    struct FakeDownloader {
        contents: Vec<u8>,
        result: Result<(), String>,
        calls: Cell<u32>,
    }

    impl Downloader for FakeDownloader {
        fn download(
            &self,
            _url: &str,
            dest: &Path,
            on_progress: &mut dyn FnMut(u64, Option<u64>),
        ) -> Result<(), String> {
            self.calls.set(self.calls.get() + 1);
            self.result.clone()?;
            let total = self.contents.len() as u64;
            on_progress(0, Some(total));
            std::fs::write(dest, &self.contents).unwrap();
            on_progress(total, Some(total));
            Ok(())
        }
    }

    #[test]
    fn ensure_model_no_ops_when_already_present() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("small.pt"), b"already here").unwrap();
        let downloader = FakeDownloader {
            contents: vec![],
            result: Ok(()),
            calls: Cell::new(0),
        };
        let mut seen = Vec::new();

        let path = ensure_model(
            &downloader,
            dir.path(),
            &ModelId::Small.info(),
            &mut |d, t| seen.push((d, t)),
        )
        .unwrap();

        assert_eq!(path, dir.path().join("small.pt"));
        assert_eq!(downloader.calls.get(), 0, "should not download");
        assert!(seen.is_empty());
    }

    #[test]
    fn ensure_model_downloads_verifies_and_relays_progress() {
        let dir = tempfile::tempdir().unwrap();
        let contents = b"pretend weights".to_vec();
        // Forge a registry entry whose checksum matches the downloaded bytes so
        // the happy path is exercised without the multi-GB real weights.
        let info = ModelInfo {
            id: ModelId::Small,
            file_name: "small.pt".to_string(),
            url: "https://example/small.pt".to_string(),
            sha256: sha256_hex(&contents),
            bundled: false,
        };
        let downloader = FakeDownloader {
            contents: contents.clone(),
            result: Ok(()),
            calls: Cell::new(0),
        };
        let mut seen = Vec::new();

        let path = ensure_model(&downloader, dir.path(), &info, &mut |d, t| {
            seen.push((d, t))
        })
        .unwrap();

        assert_eq!(path, dir.path().join("small.pt"));
        assert_eq!(downloader.calls.get(), 1);
        let total = contents.len() as u64;
        assert_eq!(seen, vec![(0, Some(total)), (total, Some(total))]);
        assert_eq!(std::fs::read(&path).unwrap(), contents);
    }

    #[test]
    fn ensure_model_rejects_checksum_mismatch_and_cleans_up() {
        let dir = tempfile::tempdir().unwrap();
        // Registry checksum is the real Small one; the fake writes other bytes.
        let downloader = FakeDownloader {
            contents: b"not the real weights".to_vec(),
            result: Ok(()),
            calls: Cell::new(0),
        };
        let mut seen = Vec::new();

        let err = ensure_model(
            &downloader,
            dir.path(),
            &ModelId::Small.info(),
            &mut |d, t| seen.push((d, t)),
        )
        .unwrap_err();

        assert_eq!(downloader.calls.get(), 1);
        assert!(!seen.is_empty(), "progress should be relayed");
        assert!(matches!(err, DownloadError::ChecksumMismatch { .. }));
        assert!(!dir.path().join("small.pt").exists(), "corrupt file removed");
    }

    #[test]
    fn ensure_model_maps_fetch_failure() {
        let dir = tempfile::tempdir().unwrap();
        let downloader = FakeDownloader {
            contents: vec![],
            result: Err("connection reset".to_string()),
            calls: Cell::new(0),
        };

        let err = ensure_model(
            &downloader,
            dir.path(),
            &ModelId::Medium.info(),
            &mut |_, _| {},
        )
        .unwrap_err();
        assert!(matches!(err, DownloadError::Fetch(_)));
    }

    #[test]
    fn ensure_model_refuses_to_fetch_a_bundled_model_when_missing() {
        let dir = tempfile::tempdir().unwrap();
        let downloader = FakeDownloader {
            contents: vec![],
            result: Ok(()),
            calls: Cell::new(0),
        };

        let err = ensure_model(
            &downloader,
            dir.path(),
            &ModelId::Base.info(),
            &mut |_, _| {},
        )
        .unwrap_err();
        assert_eq!(err, DownloadError::AlreadyBundled);
        assert_eq!(downloader.calls.get(), 0);
    }
}
