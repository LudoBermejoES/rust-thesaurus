use std::io::{BufRead, BufReader, Read};
use flate2::read::GzDecoder;
use serde::Deserialize;
use crate::{Result, ThesaurusError};

/// One word-sense from the sense-structured JSONL artifact.
#[derive(Deserialize, Debug, Clone)]
pub struct JsonlSense {
    #[serde(default)]
    pub pos: Option<String>,
    #[serde(default)]
    pub definition: Option<String>,
    #[serde(default)]
    pub synonyms: Vec<String>,
    #[serde(default)]
    pub antonyms: Vec<String>,
}

/// One entry from the sense-structured JSONL artifact.
/// Schema: `{ "word": "...", "senses": [ { "pos", "definition", "synonyms", "antonyms" } ] }`
#[derive(Deserialize, Debug, Clone)]
pub struct JsonlEntry {
    pub word: String,
    #[serde(default)]
    pub senses: Vec<JsonlSense>,
}

/// Parse a gzipped JSONL file, calling `on_entry` for each valid line.
pub fn parse_gz<R: Read>(reader: R, mut on_entry: impl FnMut(JsonlEntry) -> Result<()>) -> Result<()> {
    let decoder = GzDecoder::new(reader);
    let buf = BufReader::new(decoder);
    for (i, line) in buf.lines().enumerate() {
        let line = line?;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let entry: JsonlEntry = serde_json::from_str(trimmed)
            .map_err(|source| ThesaurusError::Json { line: i + 1, source })?;
        on_entry(entry)?;
    }
    Ok(())
}
