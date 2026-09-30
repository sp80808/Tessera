//! Tokenizer adapters and the manifest that names them.
//!
//! Vocabularies come from exactly two places, never from the network:
//! 1. data embedded in a crates.io dependency (`tiktoken-rs` bundles the
//!    OpenAI r50k/p50k/cl100k/o200k vocabularies);
//! 2. files the user placed under the tokenizers directory, referenced by a
//!    relative path and pinned by SHA-256 in the manifest. A missing, unpinned
//!    or mismatching file yields an explicit `skipped` record, never silence.
//!
//! An adapter exposes one operation: text to model-token byte ranges. Counts,
//! alignment and stability metrics are all derived from those ranges.

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use serde::Deserialize;
use tiktoken_rs::CoreBPE;

use crate::corpus::{safe_relative, sha256_hex};
use crate::json::Json;

/// Dependency versions recorded as provenance. `tests/provenance.rs` checks
/// these against `Cargo.lock` so they cannot drift silently.
pub const TIKTOKEN_RS_VERSION: &str = "0.12.1";
pub const HF_TOKENIZERS_VERSION: &str = "0.23.2";

pub const MANIFEST_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub enum Loader {
    #[serde(rename = "tiktoken-embedded")]
    TiktokenEmbedded,
    #[serde(rename = "hf-tokenizers-json")]
    HfTokenizersJson,
}

impl Loader {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::TiktokenEmbedded => "tiktoken-embedded",
            Self::HfTokenizersJson => "hf-tokenizers-json",
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EmbeddedRef {
    #[serde(rename = "crate")]
    pub krate: String,
    pub encoding: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileRef {
    pub repo_id: String,
    /// File name in the upstream repository (what a user downloads).
    pub hub_filename: String,
    /// Where the file must be placed, relative to the tokenizers directory.
    pub local_path: String,
    /// Pinned content hash; `null` until a download is approved.
    pub sha256: Option<String>,
    /// Upstream commit/revision the file was taken from; `null` until pinned.
    pub revision: Option<String>,
    pub approx_size_mb: Option<f64>,
    /// `false` while repo id / file name / size are unchecked against the hub.
    pub hub_metadata_verified: bool,
    pub license_note: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManifestEntry {
    pub id: String,
    pub family: String,
    pub vendor: String,
    pub loader: Loader,
    pub embedded: Option<EmbeddedRef>,
    pub file: Option<FileRef>,
    pub notes: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub schema_version: u32,
    pub tokenizers: Vec<ManifestEntry>,
}

/// # Errors
/// Unreadable/invalid manifest, unsupported schema version, duplicate ids,
/// or entries missing the reference their loader needs.
pub fn load_manifest(path: &Path) -> Result<Manifest, String> {
    let bytes = fs::read(path).map_err(|e| format!("cannot read tokenizer manifest: {e}"))?;
    parse_manifest(&bytes)
}

/// # Errors
/// See [`load_manifest`].
pub fn parse_manifest(bytes: &[u8]) -> Result<Manifest, String> {
    let manifest: Manifest =
        serde_json::from_slice(bytes).map_err(|e| format!("invalid tokenizer manifest: {e}"))?;
    if manifest.schema_version != MANIFEST_SCHEMA_VERSION {
        return Err(format!(
            "unsupported manifest schema_version {}",
            manifest.schema_version
        ));
    }
    let mut ids = BTreeSet::new();
    for entry in &manifest.tokenizers {
        if !ids.insert(entry.id.as_str()) {
            return Err(format!("duplicate tokenizer id `{}`", entry.id));
        }
        let ok = match entry.loader {
            Loader::TiktokenEmbedded => entry.embedded.is_some() && entry.file.is_none(),
            Loader::HfTokenizersJson => entry.file.is_some() && entry.embedded.is_none(),
        };
        if !ok {
            return Err(format!(
                "tokenizer `{}`: loader `{}` needs {} and no other reference",
                entry.id,
                entry.loader.as_str(),
                match entry.loader {
                    Loader::TiktokenEmbedded => "`embedded`",
                    Loader::HfTokenizersJson => "`file`",
                }
            ));
        }
    }
    Ok(manifest)
}

/// A loaded tokenizer: text to model-token byte ranges.
pub enum Encoder {
    Tiktoken(&'static CoreBPE),
    #[cfg(feature = "hf")]
    Hf(Box<tokenizers::Tokenizer>),
}

impl Encoder {
    /// Byte ranges `(start, end)` of each model token, in order.
    ///
    /// # Errors
    /// Backend failures, or a tiktoken tokenization that does not tile the
    /// input exactly (would indicate a broken vocabulary).
    pub fn encode_ranges(&self, text: &str) -> Result<Vec<(usize, usize)>, String> {
        match self {
            Self::Tiktoken(bpe) => {
                let mut pos = 0;
                let mut ranges = Vec::new();
                for id in bpe.encode_ordinary(text) {
                    let bytes = bpe
                        .decode_bytes(&[id])
                        .map_err(|e| format!("cannot decode token {id}: {e}"))?;
                    ranges.push((pos, pos + bytes.len()));
                    pos += bytes.len();
                }
                if pos == text.len() {
                    Ok(ranges)
                } else {
                    Err(format!(
                        "tokens cover {pos} bytes but the input has {}",
                        text.len()
                    ))
                }
            }
            #[cfg(feature = "hf")]
            Self::Hf(tokenizer) => {
                // No special tokens: the benchmark measures raw source text.
                let encoding = tokenizer.encode(text, false).map_err(|e| e.to_string())?;
                Ok(encoding.get_offsets().to_vec())
            }
        }
    }

    /// Number of model tokens.
    ///
    /// # Errors
    /// See [`Encoder::encode_ranges`].
    pub fn count(&self, text: &str) -> Result<usize, String> {
        self.encode_ranges(text).map(|r| r.len())
    }
}

/// Load a HuggingFace `tokenizers` JSON document from memory.
///
/// # Errors
/// Invalid tokenizer JSON.
#[cfg(feature = "hf")]
pub fn hf_from_bytes(bytes: &[u8]) -> Result<Encoder, String> {
    tokenizers::Tokenizer::from_bytes(bytes)
        .map(|t| Encoder::Hf(Box::new(t)))
        .map_err(|e| e.to_string())
}

/// Where a tokenizer's vocabulary and version come from.
#[derive(Debug, Clone, Default)]
pub struct Provenance {
    pub loader: String,
    pub crate_name: Option<String>,
    pub crate_version: Option<String>,
    pub encoding: Option<String>,
    pub repo_id: Option<String>,
    pub hub_filename: Option<String>,
    pub local_path: Option<String>,
    pub revision: Option<String>,
    pub sha256: Option<String>,
    pub loader_crate: Option<String>,
}

impl Provenance {
    #[must_use]
    pub fn to_json(&self) -> Json {
        Json::obj([
            ("loader", Json::str(&self.loader)),
            ("crate", Json::opt_str(self.crate_name.as_deref())),
            (
                "crate_version",
                Json::opt_str(self.crate_version.as_deref()),
            ),
            ("encoding", Json::opt_str(self.encoding.as_deref())),
            ("repo_id", Json::opt_str(self.repo_id.as_deref())),
            ("hub_filename", Json::opt_str(self.hub_filename.as_deref())),
            ("local_path", Json::opt_str(self.local_path.as_deref())),
            ("revision", Json::opt_str(self.revision.as_deref())),
            ("sha256", Json::opt_str(self.sha256.as_deref())),
            ("loader_crate", Json::opt_str(self.loader_crate.as_deref())),
        ])
    }

    /// One-line human description for the Markdown provenance table.
    #[must_use]
    pub fn describe(&self) -> String {
        if let (Some(name), Some(version), Some(encoding)) =
            (&self.crate_name, &self.crate_version, &self.encoding)
        {
            return format!("{name} {version} (embedded `{encoding}`)");
        }
        let rev = self.revision.as_deref().unwrap_or("unrecorded");
        let sha = self
            .sha256
            .as_deref()
            .map_or("unpinned", |s| &s[..s.len().min(12)]);
        format!(
            "{} `{}` rev {rev}, sha256 {sha}",
            self.repo_id.as_deref().unwrap_or("?"),
            self.hub_filename.as_deref().unwrap_or("?")
        )
    }
}

pub enum SlotState {
    Ready(Encoder),
    Skipped(String),
}

pub struct TokenizerSlot {
    pub id: String,
    pub family: String,
    pub vendor: String,
    pub notes: String,
    pub provenance: Provenance,
    pub state: SlotState,
}

impl TokenizerSlot {
    #[must_use]
    pub fn encoder(&self) -> Option<&Encoder> {
        match &self.state {
            SlotState::Ready(e) => Some(e),
            SlotState::Skipped(_) => None,
        }
    }
}

/// Resolve every manifest entry to a ready encoder or an explicit skip reason.
/// Reasons name only manifest-relative paths (no absolute paths in results).
#[must_use]
pub fn build_slots(manifest: &Manifest, tokenizers_dir: &Path) -> Vec<TokenizerSlot> {
    manifest
        .tokenizers
        .iter()
        .map(|entry| {
            let (provenance, state) = match entry.loader {
                Loader::TiktokenEmbedded => embedded_slot(entry),
                Loader::HfTokenizersJson => file_slot(entry, tokenizers_dir),
            };
            TokenizerSlot {
                id: entry.id.clone(),
                family: entry.family.clone(),
                vendor: entry.vendor.clone(),
                notes: entry.notes.clone(),
                provenance,
                state,
            }
        })
        .collect()
}

fn embedded_slot(entry: &ManifestEntry) -> (Provenance, SlotState) {
    let Some(embedded) = entry.embedded.as_ref() else {
        return (
            Provenance::default(),
            SlotState::Skipped("manifest entry has no `embedded` reference".to_owned()),
        );
    };
    let provenance = Provenance {
        loader: Loader::TiktokenEmbedded.as_str().to_owned(),
        crate_name: Some(embedded.krate.clone()),
        crate_version: Some(TIKTOKEN_RS_VERSION.to_owned()),
        encoding: Some(embedded.encoding.clone()),
        ..Provenance::default()
    };
    if embedded.krate != "tiktoken-rs" {
        return (
            provenance,
            SlotState::Skipped(format!("unsupported embedded crate `{}`", embedded.krate)),
        );
    }
    let bpe = match embedded.encoding.as_str() {
        "r50k_base" => tiktoken_rs::r50k_base_singleton(),
        "p50k_base" => tiktoken_rs::p50k_base_singleton(),
        "cl100k_base" => tiktoken_rs::cl100k_base_singleton(),
        "o200k_base" => tiktoken_rs::o200k_base_singleton(),
        other => {
            return (
                provenance,
                SlotState::Skipped(format!("unknown embedded encoding `{other}`")),
            );
        }
    };
    (provenance, SlotState::Ready(Encoder::Tiktoken(bpe)))
}

fn file_slot(entry: &ManifestEntry, dir: &Path) -> (Provenance, SlotState) {
    let Some(file) = entry.file.as_ref() else {
        return (
            Provenance::default(),
            SlotState::Skipped("manifest entry has no `file` reference".to_owned()),
        );
    };
    let provenance = Provenance {
        loader: Loader::HfTokenizersJson.as_str().to_owned(),
        repo_id: Some(file.repo_id.clone()),
        hub_filename: Some(file.hub_filename.clone()),
        local_path: Some(file.local_path.clone()),
        revision: file.revision.clone(),
        sha256: file.sha256.clone(),
        loader_crate: Some(format!("tokenizers {HF_TOKENIZERS_VERSION}")),
        ..Provenance::default()
    };
    let skip = |reason: String| (provenance.clone(), SlotState::Skipped(reason));
    if !safe_relative(&file.local_path) {
        return skip(format!("unsafe local_path `{}`", file.local_path));
    }
    let bytes = match fs::read(dir.join(&file.local_path)) {
        Ok(bytes) => bytes,
        Err(_) => {
            return skip(format!(
                "artifact not found: `{}` (not downloaded; sha256 {}, revision {})",
                file.local_path,
                if file.sha256.is_some() {
                    "pinned"
                } else {
                    "null"
                },
                if file.revision.is_some() {
                    "pinned"
                } else {
                    "null"
                },
            ));
        }
    };
    let actual = sha256_hex(&bytes);
    match file.sha256.as_deref() {
        None => {
            return skip(format!(
                "artifact `{}` present but manifest sha256 is null (unpinned); pin it (sha256 {actual}) before it can be measured",
                file.local_path
            ));
        }
        Some(pinned) if !pinned.eq_ignore_ascii_case(&actual) => {
            return skip(format!(
                "sha256 mismatch for `{}`: manifest {pinned}, file {actual}",
                file.local_path
            ));
        }
        Some(_) => {}
    }
    #[cfg(feature = "hf")]
    {
        match hf_from_bytes(&bytes) {
            Ok(encoder) => (provenance.clone(), SlotState::Ready(encoder)),
            Err(e) => skip(format!("failed to load `{}`: {e}", file.local_path)),
        }
    }
    #[cfg(not(feature = "hf"))]
    {
        skip("built without the `hf` cargo feature (HF tokenizers JSON loader disabled)".to_owned())
    }
}
