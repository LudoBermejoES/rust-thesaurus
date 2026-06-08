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

fn fixture_gz() -> Vec<u8> {
    make_gz(&[
        r#"{"word":"bonito","synonyms":["bello","hermoso"],"pos":"a","antonyms":["feo"]}"#,
        r#"{"word":"feo","synonyms":["horrible","feísimo"],"pos":"a"}"#,
        r#"{"word":"casa","synonyms":["hogar","domicilio"],"pos":"n"}"#,
        r#"{"word":"incitar","synonyms":["instigar","provocar"],"pos":"v","antonyms":[]}"#,
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

// 6.1 JSONL parse → index build; antonyms stored as 'ant'; missing field tolerated
#[test]
fn test_parse_and_index() {
    use crate::{parser, db};
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("test.db");

    let gz = fixture_gz();
    let mut entries = Vec::new();
    parser::parse_gz(std::io::Cursor::new(&gz), |e| { entries.push(e); Ok(()) }).unwrap();

    // "feo" has no antonyms field → tolerated
    assert_eq!(entries.len(), 4);
    assert!(entries[0].antonyms.contains(&"feo".to_string()));
    assert!(entries[1].antonyms.is_empty()); // feo has no antonyms

    db::build_index(&db_path, entries.into_iter()).unwrap();

    // syn rows exist
    let syns = db::synonyms(&db_path, "bonito").unwrap();
    assert!(syns.contains(&"bello".to_string()));
    assert!(syns.contains(&"hermoso".to_string()));

    // ant row for bonito exists in db but NOT returned by synonyms()
    assert!(!syns.contains(&"feo".to_string()));
}

// 6.2 synonyms() round-trip: case-insensitive, word excluded, antonyms not returned, unknown→[]
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

    // case-insensitive
    let lower = engine.synonyms("bonito").unwrap();
    let upper = engine.synonyms("Bonito").unwrap();
    assert_eq!(lower, upper);
    assert!(lower.contains(&"bello".to_string()));

    // word itself excluded
    assert!(!lower.contains(&"bonito".to_string()));

    // antonyms excluded
    assert!(!lower.contains(&"feo".to_string()));

    // unknown word → empty
    let unknown = engine.synonyms("xyzzy_nonexistent").unwrap();
    assert!(unknown.is_empty());
}

// 6.3 version.json no-op re-provision; checksum-mismatch rejection
#[test]
fn test_is_installed_version_check() {
    let dir = tempdir().unwrap();
    let config = EngineConfig {
        data_dir: dir.path().to_path_buf(),
        lang: "es".into(),
        source_url: "http://example.com".into(),
        source_sha256: "fixture".into(),
    };

    // Not installed yet
    assert!(!crate::state::is_installed_for(&config));

    // Write matching db + version
    write_fixture_db(dir.path(), "es");
    // But config has sha256="fixture" and we wrote sha256="fixture" → matches
    // Redefine config pointing at same fixture sha
    assert!(crate::state::is_installed_for(&config));

    // Checksum mismatch → not installed
    let config_wrong = EngineConfig {
        data_dir: dir.path().to_path_buf(),
        lang: "es".into(),
        source_url: "http://example.com".into(),
        source_sha256: "wrong_checksum".into(),
    };
    assert!(!crate::state::is_installed_for(&config_wrong));
}

// 6.3 (cont.) engine not ready → synonyms returns []
#[test]
fn test_synonyms_not_ready_returns_empty() {
    let dir = tempdir().unwrap();
    let config = EngineConfig {
        data_dir: dir.path().to_path_buf(),
        lang: "en".into(),
        source_url: "http://example.com".into(),
        source_sha256: "doesnotexist".into(),
    };
    let engine = ThesaurusEngine::new(config);
    assert_eq!(engine.state(), ThesaurusState::NotInstalled);
    let result = engine.synonyms("house").unwrap();
    assert!(result.is_empty());
}

// set_data_dir updates path and re-probes state
#[test]
fn test_set_data_dir_updates_path_and_state() {
    let dir1 = tempdir().unwrap();
    let dir2 = tempdir().unwrap();

    // Write a valid DB in dir2
    write_fixture_db(dir2.path(), "es");

    let config = EngineConfig {
        data_dir: dir1.path().to_path_buf(),
        lang: "es".into(),
        source_url: "http://example.com".into(),
        source_sha256: "fixture".into(),
    };
    let engine = ThesaurusEngine::new(config);
    // dir1 has no DB → NotInstalled
    assert_eq!(engine.state(), ThesaurusState::NotInstalled);

    // Redirect to dir2 which has the fixture DB
    engine.set_data_dir(dir2.path().to_path_buf());
    assert_eq!(engine.state(), ThesaurusState::Ready);
    assert!(engine.is_installed());

    // Redirect back to an empty dir → NotInstalled again
    let dir3 = tempdir().unwrap();
    engine.set_data_dir(dir3.path().to_path_buf());
    assert_eq!(engine.state(), ThesaurusState::NotInstalled);
}

// 6.4 Integration test (ignored — needs network)
#[tokio::test]
#[ignore]
async fn integration_provision_es_and_lookup() {
    let dir = tempdir().unwrap();
    let config = EngineConfig::default_es(dir.path().to_path_buf());
    let engine = ThesaurusEngine::new(config);
    assert!(!engine.is_installed());

    engine.provision(|s| println!("{:?}", s)).await.unwrap();
    assert!(engine.is_installed());

    let syns = engine.synonyms("instigar").unwrap();
    assert!(syns.contains(&"incitar".to_string()), "expected incitar in {:?}", syns);
}
