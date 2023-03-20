use ahash::RandomState;

use anyhow::Result;
use glob::{glob, Pattern};
use path_clean::PathClean;
use serde::Deserialize;
use std::io::Read;
use std::{
    collections::HashMap,
    env,
    fs::{self, File},
    path::PathBuf,
};
use toml::{from_str, Table, Value};
#[derive(Deserialize)]
pub struct StoryCrate {
    /// Contains the manifest to be passed into Cargo.
    pub cargo: Table,
    pub id: String,
}

/**
 * Options to start the build step of Minicrates
 */
pub struct BuildOptions {
    pub no_minicrates: Option<Box<dyn Fn()>>,
}

#[derive(Deserialize)]
pub struct Manifest {
    workspace: Option<Workspace>,
}

#[derive(Deserialize, Debug)]
pub struct Minicrates {
    minicrates: Option<Table>,
}

#[derive(Deserialize)]
pub struct Workspace {
    members: Vec<String>,
}
impl BuildOptions {
    pub fn build(&mut self, path: &str) -> Result<CrateIdMap> {
        let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());

        let minicrates: Option<Minicrates> = {
            let minicrates_file = PathBuf::from(&manifest_dir).join("Minicrates.toml");
            if minicrates_file.try_exists()? {
                let mut manifest = String::new();

                File::open(minicrates_file)?.read_to_string(&mut manifest)?;
                Some(toml::from_str(&manifest)?)
            } else {
                None
            }
        };

        let entries: Vec<PathBuf> = glob(path)?.map(|glob| glob.unwrap()).collect();
        if entries.len() == 0 {
            if self.no_minicrates.is_some() {
                self.no_minicrates.as_mut().unwrap()();
            }
        }
        let mut map = HashMap::new();
        for entry in entries {
            let hash_builder = RandomState::with_seed(42);
            let id = hash_builder.hash_one(&entry);
            map.insert(entry.clone(), id);
            let dist_path = manifest_dir.join("minicrates").join(
                entry
                    .file_name()
                    .unwrap()
                    .to_str()
                    .unwrap()
                    .strip_suffix(".mini.rs")
                    .unwrap(),
            );

            if dist_path.exists() {
                let metadata = dist_path.metadata()?;
                if metadata.is_dir() {
                    fs::remove_dir_all(&dist_path)?;
                } else if metadata.is_file() {
                    panic!("Error: {dist_path:?} exists but it is a file!")
                } else if metadata.is_symlink() {
                    panic!("Error: {dist_path:?} exists but it is a symbolic link!")
                }
            }
            let crate_dist = &dist_path.join(&dist_path);

            let lib_path = dist_path.join("src").join("lib.rs");
            let rel_path = pathdiff::diff_paths(&entry, &dist_path.join("src")).unwrap();
            println!("rel: {rel_path:#?} lib: {lib_path:#?} entry: {entry:#?}");
            fs::create_dir_all(lib_path.parent().unwrap())?;
            let gitignore = PathBuf::from(&manifest_dir)
                .join("minicrates")
                .join(".gitignore");
            if !gitignore.try_exists()? {
                fs::write(gitignore, "*/**")?;
            }
            fs::write(lib_path, format!("include!({rel_path:#?});"))?;

            let cargo_toml = PathBuf::from(crate_dist).join("Cargo.toml");

            let str = r#"[package]
name = "minicrates-"#
                .to_string()
                + &id.to_string()
                + r#""
[lib]
crate-type = ["dylib"]"#;
            let default: Table = toml::from_str(&str)?;
            /// Combine any any number of tables into a single one
            fn combine_tables(tables: &Vec<&Table>) -> Table {
                let mut recursive_jobs: HashMap<String, Vec<&Table>> = HashMap::new();
                let mut table = Table::new();
                for input_table in tables {
                    for (key, val) in *input_table {
                        match val {
                            Value::Table(table) => {
                                if let Some(tables) = recursive_jobs.get_mut(key) {
                                    tables.push(&table);
                                } else {
                                    recursive_jobs.insert(key.to_owned(), vec![&table]);
                                }
                            }
                            _ => {
                                table.insert(key.to_string(), val.to_owned());
                            }
                        };
                    }
                }

                for (key, tables) in recursive_jobs {
                    table.insert(key, combine_tables(&tables).into());
                }

                table
            }
            let mut tables = vec![default];
            if let Some(minicrates) = &minicrates {
                if let Some(minicrates) = &minicrates.minicrates {
                    tables.extend(
                        minicrates
                            .iter()
                            .filter(|(key, _val)| {
                                let glob = Pattern::new(&format!(
                                    "{}/{key}",
                                    manifest_dir.to_string_lossy().to_string()
                                ))
                                .unwrap()
                                .matches(entry.as_os_str().to_str().unwrap());
                                glob
                            })
                            .map(|(_key, val)| Table::try_from(val).unwrap())
                            .collect::<Vec<Table>>(),
                    );
                }
            }
            let mut table = combine_tables(&tables.iter().map(|i| i).collect());
            if let Some(dependencies) = table.get_mut("dependencies") {
                match dependencies {
                    Value::Table(table) => {
                        for (_key, value) in table {
                            if let Value::Table(table) = value {
                                if let Some(value) = table.get_mut("path") {
                                    if let Value::String(path) = value {
                                        let pathbuf = PathBuf::from(&path);
                                        *path = pathdiff::diff_paths(
                                            if pathbuf.is_absolute() {
                                                pathbuf.clean()
                                            } else {
                                                manifest_dir.join(pathbuf).clean()
                                            },
                                            &dist_path,
                                        )
                                        .unwrap()
                                        .to_string_lossy()
                                        .to_string();
                                    }
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
            fs::write(&cargo_toml, toml::to_string(&table)?)?;
        }

        // Try to look if the minicrates are added to Cargo's workspace list or not.

        let result = {
            let mut path = PathBuf::from(manifest_dir);

            let mut relative = PathBuf::new();

            let mut result = false;
            for count in 0..5 {
                let parent_manifest = path.join("Cargo.toml");
                if if parent_manifest.try_exists()? {
                    let mut string = String::new();

                    File::open(parent_manifest)?.read_to_string(&mut string)?;

                    let manifest: Manifest = from_str(&string)?;
                    if let Some(workspace) = manifest.workspace {
                        if workspace
                            .members
                            .contains(&relative.join("minicrates").to_string_lossy().to_string())
                        {
                            true
                        } else {
                            false
                        }
                    } else {
                        false
                    }
                } else {
                    false
                } {
                    result = true;
                    break;
                } else {
                    if count == 5 {
                        break;
                    } else {
                        path = path.parent().unwrap().to_owned();
                        relative.extend([path.file_name().unwrap()]);
                    }
                };
            }
            result
        };

        if !result {
            println!("cargo:warning=Minicrates has configured the minicrates for youu!! It only needs Cargo to actually compile it in order to run.");
        }

        Ok(map)
    }
}

/**
 * This is.. literally.. a hash map? In minicrates, the V is always a hash of the K...
 */
pub type CrateIdMap = HashMap<PathBuf, u64>;
