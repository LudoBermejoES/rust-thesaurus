use std::io::{BufRead, BufReader, Read};
use flate2::read::GzDecoder;
use serde::Deserialize;
use crate::{Result, ThesaurusError};

#[derive(Deserialize)]
pub struct JsonlEntry {
    pub word: String,
    pub synonyms: Vec<String>,
    pub pos: Option<String>,
    #[serde(default)]
    pub antonyms: Vec<String>,
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
