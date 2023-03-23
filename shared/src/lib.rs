use std::{collections::HashMap, path::PathBuf};


/**
 * This is.. literally.. a hash map? In minicrates, the V is always a hash of the K...
 */
pub type CrateIdMap = HashMap<PathBuf, u64>;
