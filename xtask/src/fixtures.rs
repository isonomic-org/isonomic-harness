//! `cargo xtask fixtures`: regenerate `fixtures/`.
//!
//! Writes every fixture as a JSONL generation and `fixtures/MANIFEST.json`.
//! long-thread is written too but is not committed (see `.gitignore`); its
//! digest in the manifest is checked by the tests.

use std::fs;

pub fn run() -> Result<(), String> {
    let dir = crate::workspace_root().join("fixtures");
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let all = harness_testkit::fixtures::all();
    for fixture in &all {
        let text = fixture.jsonl();
        let path = dir.join(format!("{}.jsonl", fixture.name));
        fs::write(&path, &text).map_err(|e| format!("{}: {e}", path.display()))?;
        println!(
            "{:<16} {:>6} events {:>9} bytes{}",
            fixture.name,
            fixture.events.len(),
            text.len(),
            if fixture.committed {
                ""
            } else {
                "  (not committed)"
            }
        );
    }
    let manifest = harness_testkit::fixtures::manifest(&all);
    fs::write(dir.join("MANIFEST.json"), manifest).map_err(|e| e.to_string())?;
    Ok(())
}
