use std::sync::{Arc, Mutex};
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;
use tracing::{info, warn};

use crate::{
    Inner, Result, ThesaurusError, ThesaurusState,
    db, parser,
    state::{self, VersionFile, SCHEMA_VERSION},
};

pub async fn run(
    inner: Arc<Mutex<Inner>>,
    on_progress: impl Fn(ThesaurusState) + Send + 'static,
) -> Result<()> {
    // Check if already up-to-date
    {
        let guard = inner.lock().unwrap();
        if state::is_installed_for(&guard.config) {
            info!("[thesaurus] already installed for {}", guard.config.lang);
            return Ok(());
        }
        // Ensure data dir exists
        std::fs::create_dir_all(&guard.config.data_dir)?;
    }

    let (url, sha256_expected, lang, data_dir) = {
        let g = inner.lock().unwrap();
        (
            g.config.source_url.clone(),
            g.config.source_sha256.clone(),
            g.config.lang.clone(),
            g.config.data_dir.clone(),
        )
    };

    let part_path = data_dir.join(format!("{}.jsonl.gz.part", lang));
    let db_path = data_dir.join(format!("{}.db", lang));
    let ver_path = data_dir.join(format!("{}.version.json", lang));

    // --- Download ---
    info!("[thesaurus] downloading {} from {}", lang, url);
    let client = reqwest::Client::new();
    let resp = client.get(&url).send().await?;
    let total = resp.content_length();

    set_state(&inner, ThesaurusState::Downloading { downloaded: 0, total });
    on_progress(ThesaurusState::Downloading { downloaded: 0, total });

    let mut file = tokio::fs::File::create(&part_path).await?;
    let mut hasher = Sha256::new();
    let mut downloaded: u64 = 0;

    use futures_util::StreamExt;
    let mut byte_stream = resp.bytes_stream();

    // Collect bytes while hashing and streaming to disk
    // We buffer the whole file to allow gz parsing from memory.
    let mut buf: Vec<u8> = Vec::new();
    while let Some(chunk) = byte_stream.next().await {
        let chunk = chunk?;
        hasher.update(&chunk);
        downloaded += chunk.len() as u64;
        buf.extend_from_slice(&chunk);
        file.write_all(&chunk).await?;
        let s = ThesaurusState::Downloading { downloaded, total };
        set_state(&inner, s.clone());
        on_progress(s);
    }
    file.flush().await?;
    drop(file);

    // --- Verify checksum ---
    let actual = format!("{:x}", hasher.finalize());
    if actual != sha256_expected {
        let _ = std::fs::remove_file(&part_path);
        warn!("[thesaurus] checksum mismatch for {}: expected {} got {}", lang, sha256_expected, actual);
        let err = ThesaurusError::ChecksumMismatch {
            expected: sha256_expected,
            actual,
        };
        set_state(&inner, ThesaurusState::Error { message: err.to_string() });
        return Err(err);
    }
    info!("[thesaurus] checksum ok for {}", lang);

    // --- Index ---
    set_state(&inner, ThesaurusState::Indexing);
    on_progress(ThesaurusState::Indexing);

    // Remove stale db if present
    if db_path.exists() {
        std::fs::remove_file(&db_path)?;
    }

    let entries: Result<Vec<_>> = {
        let cursor = std::io::Cursor::new(&buf);
        let mut entries = Vec::new();
        parser::parse_gz(cursor, |e| { entries.push(e); Ok(()) })?;
        Ok(entries)
    };
    let entries = entries?;

    db::build_index(&db_path, entries.into_iter())?;

    // --- Write version file ---
    let version = VersionFile {
        lang: lang.clone(),
        source_sha256: sha256_expected,
        schema_version: SCHEMA_VERSION,
    };
    std::fs::write(&ver_path, serde_json::to_string_pretty(&version).unwrap())?;

    // --- Discard downloaded file ---
    let _ = std::fs::remove_file(&part_path);

    set_state(&inner, ThesaurusState::Ready);
    on_progress(ThesaurusState::Ready);
    info!("[thesaurus] provision complete for {}", lang);
    Ok(())
}

fn set_state(inner: &Arc<Mutex<Inner>>, state: ThesaurusState) {
    inner.lock().unwrap().state = state;
}
