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

/// One word-sense returned by `lookup`.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ThesaurusSense {
    pub pos: Option<String>,
    pub definition: Option<String>,
    pub synonyms: Vec<String>,
    pub antonyms: Vec<String>,
}

/// Sense-structured lookup result returned by `ThesaurusEngine::lookup`.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ThesaurusEntry {
    pub word: String,
    /// Set by the command layer when senses came from the lemma rather than the
    /// surface word. `crate::lookup` always returns `None` — the crate is
    /// lemmatizer-agnostic; provenance is the command's responsibility.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lemma: Option<String>,
    pub senses: Vec<ThesaurusSense>,
}

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
    /// SHA-256 is updated once the new sense-structured artifact is published (task 3.4).
    pub fn default_en(data_dir: PathBuf) -> Self {
        Self {
            data_dir,
            lang: "en".into(),
            source_url: "https://raw.githubusercontent.com/LudoBermejoES/corylus-thesaurus/master/thesaurus/derived/en_dict.jsonl.gz".into(),
            source_sha256: "37b0cbc8fd61cb7f2d809ab563b937b58d8c5ae63bb3a7c1b88e22ae58d9439c".into(),
        }
    }

    /// Default config for Spanish using the corylus-thesaurus public repo.
    /// Source: Spanish Wiktionary edition (eswiktionary via Kaikki) — definitions in Spanish.
    pub fn default_es(data_dir: PathBuf) -> Self {
        Self {
            data_dir,
            lang: "es".into(),
            source_url: "https://raw.githubusercontent.com/LudoBermejoES/corylus-thesaurus/master/thesaurus/derived/es_dict.jsonl.gz".into(),
            source_sha256: "478b979f2e78283a3edc03b23b35d683b31177877cd7aca90ee4cc8c17f64d57".into(),
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

    /// Returns the current data directory used by this engine.
    pub fn data_dir(&self) -> PathBuf {
        self.inner.lock().unwrap().config.data_dir.clone()
    }

    /// Replace the data directory and re-probe install state.
    ///
    /// Call this from `setup()` once the app handle is available, passing the
    /// bundle-scoped per-user application-data directory resolved via
    /// `app.path().app_data_dir()`. The engine uses `data_dir` directly (no
    /// sub-directory append); the caller is responsible for any sub-path.
    pub fn set_data_dir(&self, data_dir: PathBuf) {
        let mut inner = self.inner.lock().unwrap();
        inner.config.data_dir = data_dir;
        inner.state = if state::is_installed_for(&inner.config) {
            ThesaurusState::Ready
        } else {
            ThesaurusState::NotInstalled
        };
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

    /// Sense-structured lookup. Returns an entry with empty senses for unknown
    /// words or when the engine is not ready — never errors on a miss.
    pub fn lookup(&self, word: &str) -> Result<ThesaurusEntry> {
        let inner = self.inner.lock().unwrap();
        if !matches!(inner.state, ThesaurusState::Ready) {
            return Ok(ThesaurusEntry { word: word.to_string(), lemma: None, senses: vec![] });
        }
        let db_path = state::db_path(&inner.config);
        drop(inner);
        db::lookup(&db_path, word)
    }

    /// Flat distinct synonym list (menu path). Returns `[]` when not ready.
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
