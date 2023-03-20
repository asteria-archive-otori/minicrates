use crate::CrateIdMap;

pub fn export(map: &CrateIdMap) -> anyhow::Result<()> {
    let string = serde_json::to_string(map)?;
    println!("cargo:rustc-env=MINICRATES_CRATES={string:?}");
    Ok(())
}

