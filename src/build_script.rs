use ahash::RandomState;
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};

use glob::{glob, Pattern};
use serde::Deserialize;
use std::io::Read;
use std::{
    collections::HashMap,
    env,
    fs::{self, DirEntry, File},
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
    minicrates: Option<Table>,
    workspace: Option<Workspace>,
}
#[derive(Deserialize)]
pub struct WorkspaceManifest {
    workspace: Option<Workspace>,
}

#[derive(Deserialize)]
pub struct Workspace {
    members: Vec<String>,
}
impl BuildOptions {
    pub fn build(&mut self, path: &str) {
        let multi = MultiProgress::new();

        let folder = multi.add(ProgressBar::new(128));
        folder.set_style(
            ProgressStyle::with_template("[{elapsed_precise}] {pos:>7}/{len:7} {msg}").unwrap(),
        );
        let file = multi.add(ProgressBar::new(128));

        file.set_style(
            ProgressStyle::with_template(
                "[{elapsed_precise}] {bar:40.cyan/blue} {pos:>7}/{len:7} {msg}",
            )
            .unwrap()
            .progress_chars("##-"),
        );
        let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap();

        let cargo = PathBuf::from(&manifest_dir).join("Cargo.toml");
        let mut manifest = String::new();

        File::open(cargo)
            .unwrap()
            .read_to_string(&mut manifest)
            .unwrap();
        let manifest: Manifest = toml::from_str(&manifest).unwrap();

        let entries: Vec<PathBuf> = glob(path)
            .expect("Failed to read glob pattern")
            .map(|glob| glob.unwrap())
            .collect();
        if entries.len() == 0 {
            if self.no_minicrates.is_some() {
                self.no_minicrates.as_mut().unwrap();
            }
        }

        folder.set_length(entries.len().try_into().unwrap());

        for entry in entries {
            folder.set_message(format!(
                "Installing symlinks: {:#?}",
                entry.as_path().parent()
            ));

            let hash_builder = RandomState::with_seed(42);
            let id = hash_builder.hash_one(&entry);

            let dist_path = PathBuf::from(format!(
                "{}/minicrates/{}",
                &manifest_dir,
                entry
                    .file_name()
                    .unwrap()
                    .to_str()
                    .unwrap()
                    .strip_suffix(".mini.rs")
                    .unwrap(),
            ));

            if dist_path.exists() {
                let metadata = dist_path.metadata().unwrap();
                if metadata.is_dir() {
                    fs::remove_dir_all(&dist_path).unwrap();
                } else if metadata.is_file() {
                    panic!("Error: {dist_path:?} exists but it is a file!")
                } else if metadata.is_symlink() {
                    panic!("Error: {dist_path:?} exists but it is a symbolic link!")
                }
            }
            let crate_dist = &dist_path.join(&dist_path);
            let dirs = fs::read_dir(entry.parent().unwrap()).unwrap();

            let dirs: Vec<DirEntry> = dirs.map(|entry| entry.unwrap()).collect();
            file.reset();
            file.set_length(dirs.len().try_into().unwrap());
            let lib_path = dist_path.join("src").join("lib.rs");
            let rel_path = pathdiff::diff_paths(&entry, &lib_path).unwrap();
            println!("rel: {rel_path:#?} lib: {lib_path:#?} entry: {entry:#?}");
            fs::create_dir_all(lib_path.parent().unwrap()).unwrap();
            let gitignore = PathBuf::from(&manifest_dir)
                .join("minicrates")
                .join(".gitignore");
            if !gitignore.try_exists().unwrap() {
                fs::write(gitignore, "*/**").unwrap();
            }
            fs::write(lib_path, format!("include!({rel_path:#?});")).unwrap();

            let cargo_toml = PathBuf::from(crate_dist).join("Cargo.toml");

            let str = r#"[package]
name = ""#
                .to_string()
                + &id.to_string()
                + r#".minicrate"
[lib]
crate-type = ["dylib"]"#;
            let default: Table = toml::from_str(&str).unwrap();
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
            if let Some(minicrates) = &manifest.minicrates {
                tables.extend(
                    minicrates
                        .iter()
                        .filter(|(key, _val)| {
                            Pattern::new(&format!("{}/{key}", manifest_dir))
                                .unwrap()
                                .matches(entry.as_os_str().to_str().unwrap())
                        })
                        .map(|(_key, val)| Table::try_from(val).unwrap())
                        .collect::<Vec<Table>>(),
                );
            }
            let table = combine_tables(&tables.iter().map(|i| i).collect());

            fs::write(&cargo_toml, toml::to_string(&table).unwrap()).unwrap();

            folder.inc(1);
        }

        // Try to look if the minicrates are added to Cargo's workspace list or not.

        let result = if let Some(workspace) = manifest.workspace {
            workspace.members.contains(&"minicrates".to_string())
        } else {
            let mut path = PathBuf::from(manifest_dir);

            let mut relative = PathBuf::new();
            (|| {
                let mut result = false;
                for count in 0..5 {
                    path = path.parent().unwrap().to_owned();
                    let parent_manifest = path.join("Cargo.toml");
                    if if parent_manifest.try_exists().unwrap() {
                        let mut string = String::new();

                        File::open(parent_manifest)
                            .unwrap()
                            .read_to_string(&mut string)
                            .unwrap();

                        let manifest: WorkspaceManifest = from_str(&string).unwrap();
                        if let Some(workspace) = manifest.workspace {
                            if workspace.members.contains(
                                &relative.join("minicrates").to_string_lossy().to_string(),
                            ) {
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
                        relative.extend([path.file_name().unwrap()]);
                        if count == 5 {
                            break;
                        }
                    };
                }
                result
            })()
        };

        if !result {
            println!("cargo:warning=Minicrates has configured the minicrates for youu!! It only needs Cargo to actually compile it in order to run.");
        }
    }
}
