//! `cargo xtask <command>`: repository tasks.
//!
//! xtask is the only crate in this repository allowed to touch files and
//! processes. Library crates stay pure; `cargo xtask deps` enforces that.

mod deps;
mod fixtures;
mod replay;
mod report;
mod smoke;

use std::process::ExitCode;

const USAGE: &str = "usage: cargo xtask <command>

commands:
  deps       check the no-I/O dependency allow-list and source scan for every library crate
  fixtures   regenerate fixtures/ (the six recorded fixtures and MANIFEST.json)
  replay     rebuild every model request of every fixture from its log [fixture]
  report     the gate's performance budgets (overhead per step, replay time)
  smoke      a two-step turn with a tool call against a local model server";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("deps") => deps::run(),
        Some("fixtures") => fixtures::run(),
        Some("replay") => replay::run(args.get(1).map(String::as_str)),
        Some("report") => report::run(),
        Some("smoke") => smoke::run(),
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
