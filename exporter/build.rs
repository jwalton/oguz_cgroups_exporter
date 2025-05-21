#![allow(missing_docs)]
use std::io::Result;

use cgroups_exporter_config::Config;

fn main() -> Result<()> {
    cargo_emit::rerun_if_changed!("../config/src/config.rs", "../config/src/lib.rs",);
    let out_dir = std::env::var("OUT_DIR").unwrap();
    let config_schema = schemars::schema_for!(Config);
    let config_schema_json = serde_json::to_string_pretty(&config_schema).unwrap();
    // Place the generated schema json file under `target/debug/` or the equivalent.
    let config_schema_path = std::path::Path::new(&out_dir)
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("config_schema.json");
    std::fs::write(&config_schema_path, config_schema_json)?;
    Ok(())
}
