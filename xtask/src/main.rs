//! `cargo xtask <command>`: repository tasks.
//!
//! xtask is the only crate in this repository allowed to touch files and
//! processes. Library crates stay pure; `cargo xtask deps` enforces that.

mod deps;
mod fixtures;

use std::process::ExitCode;

const USAGE: &str = "usage: cargo xtask <command>

commands:
  deps       check the no-I/O dependency allow-list and source scan for every library crate
  fixtures   regenerate fixtures/ (the six recorded fixtures and MANIFEST.json)";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("deps") => deps::run(),
        Some("fixtures") => fixtures::run(),
        _ => {
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("error: {message}");
            ExitCode::FAILURE
        }
    }
}

/// The workspace root: the parent of this crate's directory.
pub fn workspace_root() -> std::path::PathBuf {
    let here = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    here.parent()
        .expect("xtask lives one level below the root")
        .to_path_buf()
}
