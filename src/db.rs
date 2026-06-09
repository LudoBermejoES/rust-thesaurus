use std::path::Path;
use rusqlite::{Connection, params};
use crate::{Result, parser::JsonlEntry};

pub fn build_index(db_path: &Path, entries: impl Iterator<Item = JsonlEntry>) -> Result<()> {
    std::fs::create_dir_all(db_path.parent().unwrap_or(Path::new(".")))?;
    let mut conn = Connection::open(db_path)?;
    // NORMAL is safe under WAL and avoids an fsync per commit; the table is
    // created without its index so bulk inserts don't pay per-row b-tree
    // maintenance — the index is built once after the load below.
    conn.execute_batch("
        PRAGMA journal_mode = WAL;
        PRAGMA synchronous = NORMAL;
        CREATE TABLE IF NOT EXISTS relations (
            word    TEXT NOT NULL,
            related TEXT NOT NULL,
            pos     TEXT,
            kind    TEXT NOT NULL
        );
    ")?;

    // Wrap every insert in a single transaction. Without this each INSERT
    // autocommits (its own fsync), which makes a multi-hundred-thousand-row
    // load effectively never finish — it manifests as a permanent "Indexing".
    let tx = conn.transaction()?;
    {
        let mut stmt = tx.prepare(
            "INSERT INTO relations (word, related, pos, kind) VALUES (?1, ?2, ?3, ?4)"
        )?;
        for entry in entries {
            let pos = entry.pos.as_deref();
            for syn in &entry.synonyms {
                if !syn.is_empty() && syn != &entry.word {
                    stmt.execute(params![entry.word, syn, pos, "syn"])?;
                }
            }
            for ant in &entry.antonyms {
                if !ant.is_empty() && ant != &entry.word {
                    stmt.execute(params![entry.word, ant, pos, "ant"])?;
                }
            }
        }
    }
    tx.commit()?;

    // Build the lookup index once, after the bulk load.
    conn.execute_batch(
        "CREATE INDEX IF NOT EXISTS idx_word ON relations(word COLLATE NOCASE);",
    )?;
    Ok(())
}

pub fn synonyms(db_path: &Path, word: &str) -> Result<Vec<String>> {
    let conn = Connection::open(db_path)?;
    let mut stmt = conn.prepare(
        "SELECT DISTINCT related FROM relations \
         WHERE word = ?1 COLLATE NOCASE AND kind = 'syn' AND related <> ?1"
    )?;
    let rows = stmt.query_map(params![word], |row| row.get::<_, String>(0))?;
    let mut result = Vec::new();
    for r in rows {
        result.push(r?);
    }
    Ok(result)
}
