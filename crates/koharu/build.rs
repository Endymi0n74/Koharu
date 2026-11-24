use std::path::Path;

fn main() {
    // `generate_context!` embeds the frontend from `packages/koharu/out`, which rustc does not
    // track as an input: without this, a frontend-only change would ship the previous bundle.
    let out = Path::new("../../packages/koharu/out");
    println!("cargo:rerun-if-changed={}", out.display());
    tauri_build::build();
}
