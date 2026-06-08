# rust-thesaurus

Standalone Rust crate that provisions and queries a per-language WordNet-derived synonym index.
No Tauri, Vue, or Corylus dependency. Data is downloaded on demand — nothing is bundled.

## Architecture

```
provision(lang)
    │
    ▼
Download <lang>_synonyms.jsonl.gz   (from corylus-thesaurus public repo)
    │
    ▼
Verify SHA-256                       (pinned per language)
    │
    ▼
Parse gzipped JSONL line by line     (single parser for en + es)
{"word","synonyms":[...],"pos","antonyms":[...]}
    │
    ▼
Build SQLite index                   (<datadir>/thesaurus/<lang>.db)
relations(word, related, pos, kind)  kind ∈ {syn, ant}
idx_word COLLATE NOCASE
    │
    ▼
Write <lang>.version.json            (lang, source sha256, schema_version)
Discard downloaded .gz file
```

## Install flow states

```
NotInstalled → Downloading{downloaded, total} → Indexing → Ready
                                                           ↕
                                                         Error{message}
```

`ThesaurusEngine::new()` probes disk on construction: if the db + version file match the
configured checksum, the engine starts in `Ready` without re-downloading.

## Data directory layout

```
<OS app data>/thesaurus/
  ├── en.db               SQLite synonym index (absent until provisioned)
  ├── es.db
  ├── en.version.json     {lang, source_sha256, schema_version}
  └── es.version.json
```

Resolved via the `dirs` crate (`data_dir()`). The host (Corylus) supplies the `data_dir` path
via `EngineConfig`; the crate itself does not call `dirs` internally.

## Usage

```rust
use rust_thesaurus::{ThesaurusEngine, EngineConfig};
use std::path::PathBuf;

let data_dir = dirs::data_dir().unwrap().join("Corylus");
let engine = ThesaurusEngine::new(EngineConfig::default_es(data_dir));

if !engine.is_installed() {
    engine.provision(|state| println!("{:?}", state)).await?;
}

let syns = engine.synonyms("bonito")?;
println!("{:?}", syns);
```

## Data sources

| Language | Source | License | Words |
|----------|--------|---------|-------|
| English  | Princeton WordNet 3.0 WNdb | WordNet License (permissive) | 111,539 |
| Spanish  | Dictionary-quality thesaurus | GPLv3 (never bundled) | 26,043 |

Artifacts hosted at: <https://github.com/LudoBermejoES/corylus-thesaurus>

Checksums:
- `en_synonyms.jsonl.gz` — `20190ad431107215dccd234f0787fbcf7edb14e0c389fb507a72891aab673f23`
- `es_synonyms.jsonl.gz` — `10f85fb3360e3a97ec1631ab3fdbfa013ef986012c373f0e74c0ee49d93b20ef`
