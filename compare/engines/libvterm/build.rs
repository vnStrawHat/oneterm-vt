//! Compiles libvterm (the checkout `compare/fetch-engines.sh` made at the pinned commit) and
//! the flat C interface the adapter calls.

use std::path::{Path, PathBuf};

fn main() {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let root = manifest.join("../../../.cargo-home/compare/src/libvterm");
    println!("cargo:rerun-if-changed=shim.c");
    println!(
        "cargo:rerun-if-changed={}",
        root.join("src/vterm.c").display()
    );
    let mut build = cc::Build::new();
    build
        .include(root.join("include"))
        .include(root.join("src"))
        .opt_level(3)
        .warnings(false);
    for name in [
        "encoding", "keyboard", "mouse", "parser", "pen", "screen", "state", "unicode", "vterm",
    ] {
        build.file(root.join("src").join(format!("{name}.c")));
    }
    build.file(Path::new("shim.c"));
    build.compile("vterm_shim");
}
