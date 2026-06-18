use std::path::Path;
use rusqlite::{Connection, params};
use crate::{Result, parser::JsonlEntry, ThesaurusSense, ThesaurusEntry};

pub fn build_index(db_path: &Path, entries: impl Iterator<Item = JsonlEntry>) -> Result<()> {
    std::fs::create_dir_all(db_path.parent().unwrap_or(Path::new(".")))?;
    let mut conn = Connection::open(db_path)?;
    conn.execute_batch("
        PRAGMA journal_mode = WAL;
        PRAGMA synchronous = NORMAL;
        CREATE TABLE IF NOT EXISTS senses (
            word      TEXT NOT NULL,
            sense_idx INTEGER NOT NULL,
            pos       TEXT,
            definition TEXT
        );
        CREATE TABLE IF NOT EXISTS relations (
            word      TEXT NOT NULL,
            related   TEXT NOT NULL,
            pos       TEXT,
            kind      TEXT NOT NULL,
            sense_idx INTEGER NOT NULL DEFAULT 0
        );
    ")?;

    let tx = conn.transaction()?;
    {
        let mut sense_stmt = tx.prepare(
            "INSERT INTO senses (word, sense_idx, pos, definition) VALUES (?1, ?2, ?3, ?4)"
        )?;
        let mut rel_stmt = tx.prepare(
            "INSERT INTO relations (word, related, pos, kind, sense_idx) VALUES (?1, ?2, ?3, ?4, ?5)"
        )?;

        for entry in entries {
            for (idx, sense) in entry.senses.iter().enumerate() {
                let sense_idx = idx as i64;
                let pos = sense.pos.as_deref();
                let def = sense.definition.as_deref();

                // Only write a senses row if there's a pos or definition to store
                if pos.is_some() || def.is_some() {
                    sense_stmt.execute(params![entry.word, sense_idx, pos, def])?;
                }

                for syn in &sense.synonyms {
                    if !syn.is_empty() && syn != &entry.word {
                        rel_stmt.execute(params![entry.word, syn, pos, "syn", sense_idx])?;
                    }
                }
                for ant in &sense.antonyms {
                    if !ant.is_empty() && ant != &entry.word {
                        rel_stmt.execute(params![entry.word, ant, pos, "ant", sense_idx])?;
                    }
                }
            }
        }
    }
    tx.commit()?;

    conn.execute_batch("
        CREATE INDEX IF NOT EXISTS idx_word ON relations(word COLLATE NOCASE);
        CREATE INDEX IF NOT EXISTS idx_senses_word ON senses(word COLLATE NOCASE);
    ")?;
    Ok(())
}

/// Full sense-structured lookup: returns one `ThesaurusEntry` per word.
/// Case-insensitive. Returns an entry with empty senses for unknown words.
pub fn lookup(db_path: &Path, word: &str) -> Result<ThesaurusEntry> {
    let conn = Connection::open(db_path)?;

    // Get all distinct sense_idx values for this word (from both tables)
    let mut idx_stmt = conn.prepare(
        "SELECT DISTINCT sense_idx FROM relations WHERE word = ?1 COLLATE NOCASE
         UNION
         SELECT DISTINCT sense_idx FROM senses WHERE word = ?1 COLLATE NOCASE
         ORDER BY sense_idx"
    )?;
    let sense_indices: Vec<i64> = idx_stmt
        .query_map(params![word], |row| row.get(0))?
        .collect::<std::result::Result<_, _>>()?;

    // Resolve canonical casing from the DB
    let canonical: String = {
        let mut stmt = conn.prepare(
            "SELECT word FROM relations WHERE word = ?1 COLLATE NOCASE LIMIT 1"
        )?;
        stmt.query_row(params![word], |r| r.get(0))
            .unwrap_or_else(|_| word.to_string())
    };

    let mut senses: Vec<ThesaurusSense> = Vec::new();

    for idx in sense_indices {
        // Get pos + definition for this sense (may not exist if only relations)
        let meta: (Option<String>, Option<String>) = {
            let mut stmt = conn.prepare(
                "SELECT pos, definition FROM senses WHERE word = ?1 COLLATE NOCASE AND sense_idx = ?2 LIMIT 1"
            )?;
            stmt.query_row(params![word, idx], |r| Ok((r.get(0)?, r.get(1)?)))
                .unwrap_or((None, None))
        };

        let mut syn_stmt = conn.prepare(
            "SELECT DISTINCT related FROM relations \
             WHERE word = ?1 COLLATE NOCASE AND kind = 'syn' AND sense_idx = ?2 AND related <> ?1"
        )?;
        let synonyms: Vec<String> = syn_stmt
            .query_map(params![word, idx], |r| r.get(0))?
            .collect::<std::result::Result<_, _>>()?;

        let mut ant_stmt = conn.prepare(
            "SELECT DISTINCT related FROM relations \
             WHERE word = ?1 COLLATE NOCASE AND kind = 'ant' AND sense_idx = ?2 AND related <> ?1"
        )?;
        let antonyms: Vec<String> = ant_stmt
            .query_map(params![word, idx], |r| r.get(0))?
            .collect::<std::result::Result<_, _>>()?;

        senses.push(ThesaurusSense {
            pos: meta.0,
            definition: meta.1,
            synonyms,
            antonyms,
        });
    }

    Ok(ThesaurusEntry { word: canonical, senses })
}

/// Flat distinct synonyms for a word (menu path — ignores sense structure).
pub fn synonyms(db_path: &Path, word: &str) -> Result<Vec<String>> {
    let conn = Connection::open(db_path)?;
    let mut stmt = conn.prepare(
        "SELECT DISTINCT related FROM relations \
         WHERE word = ?1 COLLATE NOCASE AND kind = 'syn' AND related <> ?1"
    )?;
    let rows = stmt.query_map(params![word], |row| row.get::<_, String>(0))?;
    Ok(rows.collect::<std::result::Result<_, _>>()?)
}

/// Flat distinct antonyms for a word (menu path — ignores sense structure).
pub fn antonyms(db_path: &Path, word: &str) -> Result<Vec<String>> {
    let conn = Connection::open(db_path)?;
    let mut stmt = conn.prepare(
        "SELECT DISTINCT related FROM relations \
         WHERE word = ?1 COLLATE NOCASE AND kind = 'ant' AND related <> ?1"
    )?;
    let rows = stmt.query_map(params![word], |row| row.get::<_, String>(0))?;
    Ok(rows.collect::<std::result::Result<_, _>>()?)
}
