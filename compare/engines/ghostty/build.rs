//! Links the static libghostty-vt that `compare/fetch-engines.sh` built.

use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-env-changed=LIBGHOSTTY_VT_DIR");
    let default = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../.cargo-home/compare/libghostty-vt");
    let dir = std::env::var_os("LIBGHOSTTY_VT_DIR").map_or(default, PathBuf::from);
    let lib = dir.join("lib");
    println!(
        "cargo:rerun-if-changed={}",
        lib.join("ghostty-vt-static.lib").display()
    );
    println!("cargo:rustc-link-search=native={}", lib.display());
    println!("cargo:rustc-link-lib=static=ghostty-vt-static");
}
