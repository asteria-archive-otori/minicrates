use std::{env, process::exit};

fn main() {
    minicrates::BuildOptions {
        no_minicrates: Some(Box::new(|| {
            println!("Nothing to build.");
            exit(0);
        })),
    }
    .build(&format!(
        "{}/story/**/*.mini.rs",
        env::var("CARGO_MANIFEST_DIR").unwrap()
    ));
}
