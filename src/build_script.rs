use ahash::RandomState;
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};

use glob::{glob, Paths, Pattern};
use serde::Deserialize;
use std::io::Read;
use std::{
    collections::HashMap,
    env,
    fs::{self, DirEntry, File},
    io::Write,
    iter::Map,
    path::PathBuf,
    process::{exit, Command},
};
use toml::{Table, Value};
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

        let entries: Vec<PathBuf> = glob(&format!("{}/${path}", &manifest_dir))
            .expect("Failed to read glob pattern")
            .map(|glob| glob.unwrap())
            .collect();
        if entries.len() == 0 {
            if self.no_minicrates.is_some() {
                self.no_minicrates.as_mut().unwrap();
            }
        }
        let out_dir = env::var("OUT_DIR").unwrap();
        folder.set_length(entries.len().try_into().unwrap());

        for entry in entries {
            folder.set_message(format!(
                "Installing symlinks: {:#?}",
                entry.as_path().parent()
            ));

            let hash_builder = RandomState::with_seed(42);
            let id = hash_builder.hash_one(&entry);

            let dist_path = PathBuf::from(format!(
                "{}/.flara/story-{:#?}-{:?}",
                &out_dir,
                entry.file_name().unwrap(),
                entry
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

            for path in dirs {
                let entry = path.path();
                let link = if if let Some(extension) = entry.extension() {
                    if extension == ".rs" {
                        true
                    } else {
                        false
                    }
                } else {
                    false
                } {
                    crate_dist.join("src")
                } else {
                    crate_dist.to_owned()
                }
                .join(entry.file_name().unwrap());

                if !link.exists() {
                    if !link.parent().unwrap().exists() {
                        fs::create_dir_all(link.parent().unwrap()).unwrap();
                    }
                    #[cfg(target_os = "windows")]
                    {
                        let metadata = path.metadata().unwrap();
                        if metadata.is_file() {
                            std::os::windows::fs::symlink_file(path.path(), link);
                        } else if metadata.is_folder() {
                            std::os::windows::fs::symlink_dir(path.path(), link);
                        }
                    }
                    println!("cargo:warning={link:#?}");
                    println!("cargo:warning={entry:#?}");

                    #[cfg(unix)]
                    std::os::unix::fs::symlink(entry, link).unwrap();
                }
                file.inc(1);
            }

            let cargo_toml = PathBuf::from(crate_dist).join("Cargo.toml");

            let str = r#"[package]
name = ""#
                .to_string()
                + &id.to_string()
                + r#".flarastory"
[lib]
crate-type = ["dylib"]
                
[dependencies]
framework = { version = "0.1.0", path = ""#
                + &manifest_dir
                + r#"/framework" }
bevy = { version = "0.9.1", features = ["dynamic"] }
bevy_rpg = { version = "0.1.0", path = ""#
                + &manifest_dir
                + r#"/bevy-rpg" }"#;
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
            if !cargo_toml.exists() {
                fs::create_dir_all(cargo_toml.parent().unwrap()).unwrap();
                File::create(&cargo_toml).unwrap();
            }

            fs::write(&cargo_toml, toml::to_string(&table).unwrap()).unwrap();
            println!("cargo:warning={cargo_toml:#?}");

            folder.inc(1);
        }
    }
}
