use anyhow::Result;
use minicrates_shared::CrateIdMap;
pub fn get() -> Result<CrateIdMap> {
    Ok(serde_json::from_str(&std::env::var("MINICRATE_CRATES")?)?)
}

lazy_static::lazy_static! {
    pub static ref MINICRATES: CrateIdMap = {
        get().unwrap()
    };
}

#[cfg(test)]
pub mod test {
    use crate::MINICRATES;

    #[test]
    pub fn test() {
        println!("{:#?}", *MINICRATES);
    }
}
