use std::process::exit;

fn main() {
    minicrates::BuildOptions {
        no_minicrates: Some(Box::new(|| {
            println!("Nothing to build.");
            exit(0);
        })),
    }.build("story")
}
