use std::path::PathBuf;
use crate::EngineConfig;

pub fn db_path(config: &EngineConfig) -> PathBuf {
    config.data_dir.join(format!("{}.db", config.lang))
}

pub fn version_path(config: &EngineConfig) -> PathBuf {
    config.data_dir.join(format!("{}.version.json", config.lang))
}

#[allow(dead_code)]
pub fn part_path(config: &EngineConfig) -> PathBuf {
    config.data_dir.join(format!("{}.jsonl.gz.part", config.lang))
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct VersionFile {
    pub lang: String,
    pub source_sha256: String,
    pub schema_version: u32,
}

pub const SCHEMA_VERSION: u32 = 2;

pub fn is_installed_for(config: &EngineConfig) -> bool {
    let db = db_path(config);
    let ver = version_path(config);
    if !db.exists() || !ver.exists() {
        return false;
    }
    let Ok(data) = std::fs::read_to_string(&ver) else { return false; };
    let Ok(v) = serde_json::from_str::<VersionFile>(&data) else { return false; };
    v.lang == config.lang
        && v.source_sha256 == config.source_sha256
        && v.schema_version == SCHEMA_VERSION
}
