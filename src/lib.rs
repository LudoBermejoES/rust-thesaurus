mod error;
mod provision;
mod parser;
mod db;
mod state;

#[cfg(test)]
mod tests;

pub use error::ThesaurusError;

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

pub type Result<T> = std::result::Result<T, ThesaurusError>;

/// Configuration for one language's thesaurus engine.
pub struct EngineConfig {
    /// Directory where per-language .db and .version.json files are stored.
    pub data_dir: PathBuf,
    /// Language code: "en" or "es".
    pub lang: String,
    /// URL of the pinned gzipped JSONL artifact.
    pub source_url: String,
    /// Pinned SHA-256 hex string of the artifact.
    pub source_sha256: String,
}

impl EngineConfig {
    /// Default config for English using the corylus-thesaurus public repo.
    pub fn default_en(data_dir: PathBuf) -> Self {
        Self {
            data_dir,
            lang: "en".into(),
            source_url: "https://raw.githubusercontent.com/LudoBermejoES/corylus-thesaurus/master/thesaurus/derived/en_synonyms.jsonl.gz".into(),
            source_sha256: "20190ad431107215dccd234f0787fbcf7edb14e0c389fb507a72891aab673f23".into(),
        }
    }

    /// Default config for Spanish using the corylus-thesaurus public repo.
    pub fn default_es(data_dir: PathBuf) -> Self {
        Self {
            data_dir,
            lang: "es".into(),
            source_url: "https://raw.githubusercontent.com/LudoBermejoES/corylus-thesaurus/master/thesaurus/derived/es_synonyms.jsonl.gz".into(),
            source_sha256: "10f85fb3360e3a97ec1631ab3fdbfa013ef986012c373f0e74c0ee49d93b20ef".into(),
        }
    }
}

/// Observable state of a thesaurus engine for one language.
#[derive(Clone, Debug, PartialEq)]
pub enum ThesaurusState {
    NotInstalled,
    Downloading { downloaded: u64, total: Option<u64> },
    Indexing,
    Ready,
    Error { message: String },
}

pub(crate) struct Inner {
    pub config: EngineConfig,
    pub state: ThesaurusState,
}

/// Per-language thesaurus engine. One instance per language.
#[derive(Clone)]
pub struct ThesaurusEngine {
    pub(crate) inner: Arc<Mutex<Inner>>,
}

impl ThesaurusEngine {
    pub fn new(config: EngineConfig) -> Self {
        let initial_state = if state::is_installed_for(&config) {
            ThesaurusState::Ready
        } else {
            ThesaurusState::NotInstalled
        };
        Self {
            inner: Arc::new(Mutex::new(Inner { config, state: initial_state })),
        }
    }

    pub fn state(&self) -> ThesaurusState {
        self.inner.lock().unwrap().state.clone()
    }

    pub fn is_installed(&self) -> bool {
        matches!(self.state(), ThesaurusState::Ready)
    }

    /// Download → verify → parse → build SQLite index.
    /// Emits `Downloading{..}` and `Indexing` via the callback.
    pub async fn provision(&self, on_progress: impl Fn(ThesaurusState) + Send + 'static) -> Result<()> {
        provision::run(self.inner.clone(), on_progress).await
    }

    /// Synchronous case-insensitive synonym lookup.
    /// Returns `[]` for unknown words or when not installed.
    pub fn synonyms(&self, word: &str) -> Result<Vec<String>> {
        let inner = self.inner.lock().unwrap();
        if !matches!(inner.state, ThesaurusState::Ready) {
            return Ok(vec![]);
        }
        let db_path = state::db_path(&inner.config);
        drop(inner);
        db::synonyms(&db_path, word)
    }
}
