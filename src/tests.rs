use std::io::Write;
use tempfile::tempdir;
use flate2::{Compression, write::GzEncoder};

use crate::{EngineConfig, ThesaurusEngine, ThesaurusState};

fn make_gz(lines: &[&str]) -> Vec<u8> {
    let mut enc = GzEncoder::new(Vec::new(), Compression::default());
    for line in lines {
        enc.write_all(line.as_bytes()).unwrap();
        enc.write_all(b"\n").unwrap();
    }
    enc.finish().unwrap()
}

/// Sense-structured fixture — matches the new schema.
fn fixture_gz() -> Vec<u8> {
    make_gz(&[
        r#"{"word":"bonito","senses":[{"pos":"adj","definition":"De aspecto agradable.","synonyms":["bello","hermoso"],"antonyms":["feo"]}]}"#,
        r#"{"word":"feo","senses":[{"pos":"adj","definition":"De aspecto desagradable.","synonyms":["horrible","feísimo"]}]}"#,
        r#"{"word":"casa","senses":[{"pos":"n","synonyms":["hogar","domicilio"]}]}"#,
        r#"{"word":"incitar","senses":[{"pos":"v","synonyms":["instigar","provocar"],"antonyms":[]}]}"#,
        r#"{"word":"happy","senses":[{"pos":"adj","definition":"Feeling pleasure.","synonyms":["glad","joyful"],"antonyms":["sad","unhappy"]},{"pos":"adj","definition":"Lucky.","synonyms":["fortunate"]}]}"#,
    ])
}

fn write_fixture_db(dir: &std::path::Path, lang: &str) {
    use crate::{parser, db, state::{VersionFile, SCHEMA_VERSION}};

    let gz = fixture_gz();
    let db_path = dir.join(format!("{}.db", lang));
    let mut entries = Vec::new();
    parser::parse_gz(std::io::Cursor::new(&gz), |e| { entries.push(e); Ok(()) }).unwrap();
    db::build_index(&db_path, entries.into_iter()).unwrap();

    let ver = VersionFile {
        lang: lang.to_string(),
        source_sha256: "fixture".to_string(),
        schema_version: SCHEMA_VERSION,
    };
    std::fs::write(
        dir.join(format!("{}.version.json", lang)),
        serde_json::to_string(&ver).unwrap(),
    ).unwrap();
}

// Parse sense-structured JSONL → index builds both senses and relations tables
#[test]
fn test_parse_sense_structured() {
    use crate::{parser, db};
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("test.db");

    let gz = fixture_gz();
    let mut entries = Vec::new();
    parser::parse_gz(std::io::Cursor::new(&gz), |e| { entries.push(e); Ok(()) }).unwrap();

    assert_eq!(entries.len(), 5);
    // bonito has senses with definition + synonyms + antonyms
    let bonito = &entries[0];
    assert_eq!(bonito.word, "bonito");
    assert_eq!(bonito.senses.len(), 1);
    assert_eq!(bonito.senses[0].definition.as_deref(), Some("De aspecto agradable."));
    assert_eq!(bonito.senses[0].synonyms, vec!["bello", "hermoso"]);
    assert_eq!(bonito.senses[0].antonyms, vec!["feo"]);

    // incitar has empty antonyms → tolerated
    let incitar = &entries[3];
    assert!(incitar.senses[0].antonyms.is_empty());

    // happy has two senses
    let happy = &entries[4];
    assert_eq!(happy.senses.len(), 2);

    db::build_index(&db_path, entries.into_iter()).unwrap();
    // syn rows exist via flat query
    let syns = db::synonyms(&db_path, "bonito").unwrap();
    assert!(syns.contains(&"bello".to_string()));
    // antonym NOT returned by synonyms()
    assert!(!syns.contains(&"feo".to_string()));
    // antonym IS returned by antonyms()
    let ants = db::antonyms(&db_path, "bonito").unwrap();
    assert!(ants.contains(&"feo".to_string()));
}

// lookup() returns sense-structured entry with definition + syn + ant
#[test]
fn test_lookup_returns_senses() {
    let dir = tempdir().unwrap();
    write_fixture_db(dir.path(), "es");

    let config = EngineConfig {
        data_dir: dir.path().to_path_buf(),
        lang: "es".into(),
        source_url: "http://example.com".into(),
        source_sha256: "fixture".into(),
    };
    let engine = ThesaurusEngine::new(config);
    assert!(engine.is_installed());

    let entry = engine.lookup("bonito").unwrap();
    assert_eq!(entry.word, "bonito");
    assert_eq!(entry.senses.len(), 1);
    let s = &entry.senses[0];
    assert_eq!(s.pos.as_deref(), Some("adj"));
    assert_eq!(s.definition.as_deref(), Some("De aspecto agradable."));
    assert!(s.synonyms.contains(&"bello".to_string()));
    assert!(s.antonyms.contains(&"feo".to_string()));
}

// lookup() with multi-sense word returns all senses
#[test]
fn test_lookup_multi_sense() {
    let dir = tempdir().unwrap();
    write_fixture_db(dir.path(), "en");

    let config = EngineConfig {
        data_dir: dir.path().to_path_buf(),
        lang: "en".into(),
        source_url: "http://example.com".into(),
        source_sha256: "fixture".into(),
    };
    let engine = ThesaurusEngine::new(config);

    let entry = engine.lookup("happy").unwrap();
    assert_eq!(entry.senses.len(), 2);
    assert!(entry.senses[0].antonyms.contains(&"sad".to_string()));
    assert!(entry.senses[1].synonyms.contains(&"fortunate".to_string()));
    assert!(entry.senses[1].antonyms.is_empty());
}

// lookup() unknown word → empty entry, no error
#[test]
fn test_lookup_unknown_word_empty() {
    let dir = tempdir().unwrap();
    write_fixture_db(dir.path(), "es");

    let config = EngineConfig {
        data_dir: dir.path().to_path_buf(),
        lang: "es".into(),
        source_url: "http://example.com".into(),
        source_sha256: "fixture".into(),
    };
    let engine = ThesaurusEngine::new(config);
    let entry = engine.lookup("xyzzy_nonexistent").unwrap();
    assert!(entry.senses.is_empty());
}

// lookup() case-insensitive
#[test]
fn test_lookup_case_insensitive() {
    let dir = tempdir().unwrap();
    write_fixture_db(dir.path(), "es");

    let config = EngineConfig {
        data_dir: dir.path().to_path_buf(),
        lang: "es".into(),
        source_url: "http://example.com".into(),
        source_sha256: "fixture".into(),
    };
    let engine = ThesaurusEngine::new(config);
    let lower = engine.lookup("bonito").unwrap();
    let upper = engine.lookup("BONITO").unwrap();
    assert_eq!(lower.senses.len(), upper.senses.len());
    assert_eq!(lower.senses[0].synonyms, upper.senses[0].synonyms);
}

// lookup() not-ready engine → empty entry
#[test]
fn test_lookup_not_ready_returns_empty() {
    let dir = tempdir().unwrap();
    let config = EngineConfig {
        data_dir: dir.path().to_path_buf(),
        lang: "en".into(),
        source_url: "http://example.com".into(),
        source_sha256: "doesnotexist".into(),
    };
    let engine = ThesaurusEngine::new(config);
    assert_eq!(engine.state(), ThesaurusState::NotInstalled);
    let entry = engine.lookup("house").unwrap();
    assert!(entry.senses.is_empty());
}

// synonyms() round-trip: case-insensitive, word excluded, antonyms not returned, unknown→[]
#[test]
fn test_synonyms_roundtrip() {
    let dir = tempdir().unwrap();
    write_fixture_db(dir.path(), "es");

    let config = EngineConfig {
        data_dir: dir.path().to_path_buf(),
        lang: "es".into(),
        source_url: "http://example.com".into(),
        source_sha256: "fixture".into(),
    };
    let engine = ThesaurusEngine::new(config);
    assert!(engine.is_installed());

    let lower = engine.synonyms("bonito").unwrap();
    let upper = engine.synonyms("Bonito").unwrap();
    assert_eq!(lower, upper);
    assert!(lower.contains(&"bello".to_string()));
    assert!(!lower.contains(&"bonito".to_string()));
    assert!(!lower.contains(&"feo".to_string())); // antonym excluded

    let unknown = engine.synonyms("xyzzy_nonexistent").unwrap();
    assert!(unknown.is_empty());
}

// version.json check: matching sha→installed, wrong sha→not installed
#[test]
fn test_is_installed_version_check() {
    let dir = tempdir().unwrap();
    let config = EngineConfig {
        data_dir: dir.path().to_path_buf(),
        lang: "es".into(),
        source_url: "http://example.com".into(),
        source_sha256: "fixture".into(),
    };
    assert!(!crate::state::is_installed_for(&config));
    write_fixture_db(dir.path(), "es");
    assert!(crate::state::is_installed_for(&config));

    let config_wrong = EngineConfig {
        data_dir: dir.path().to_path_buf(),
        lang: "es".into(),
        source_url: "http://example.com".into(),
        source_sha256: "wrong_checksum".into(),
    };
    assert!(!crate::state::is_installed_for(&config_wrong));
}

// set_data_dir updates path and re-probes state
#[test]
fn test_set_data_dir_updates_path_and_state() {
    let dir1 = tempdir().unwrap();
    let dir2 = tempdir().unwrap();
    write_fixture_db(dir2.path(), "es");

    let config = EngineConfig {
        data_dir: dir1.path().to_path_buf(),
        lang: "es".into(),
        source_url: "http://example.com".into(),
        source_sha256: "fixture".into(),
    };
    let engine = ThesaurusEngine::new(config);
    assert_eq!(engine.state(), ThesaurusState::NotInstalled);

    engine.set_data_dir(dir2.path().to_path_buf());
    assert_eq!(engine.state(), ThesaurusState::Ready);

    let dir3 = tempdir().unwrap();
    engine.set_data_dir(dir3.path().to_path_buf());
    assert_eq!(engine.state(), ThesaurusState::NotInstalled);
}

// Integration test (ignored — needs network)
#[tokio::test]
#[ignore]
async fn integration_provision_es_and_lookup() {
    let dir = tempdir().unwrap();
    let config = EngineConfig::default_es(dir.path().to_path_buf());
    let engine = ThesaurusEngine::new(config);
    assert!(!engine.is_installed());

    engine.provision(|s| println!("{:?}", s)).await.unwrap();
    assert!(engine.is_installed());

    let entry = engine.lookup("libre").unwrap();
    assert!(!entry.senses.is_empty(), "expected senses for 'libre': {:?}", entry);
}
