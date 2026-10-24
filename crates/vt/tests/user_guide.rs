//! Guards on the user guide that doctests cannot give: every chapter is wired into the doctest
//! run, and the API map names every public type. Reads files only; uses no crate API.

use std::fs;
use std::path::{Path, PathBuf};

fn manifest_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn user_docs_dir() -> PathBuf {
    manifest_dir().join("../../docs/user")
}

fn read(path: &Path) -> String {
    fs::read_to_string(path).unwrap_or_else(|err| panic!("cannot read {}: {err}", path.display()))
}

#[test]
fn every_user_doc_is_a_doctest_module() {
    let lib = read(&manifest_dir().join("src/lib.rs"));
    let mut missing = Vec::new();
    let entries = fs::read_dir(user_docs_dir()).expect("docs/user must exist");
    for entry in entries {
        let name = entry.expect("readable directory entry").file_name();
        let name = name.to_string_lossy();
        if name.ends_with(".md")
            && !lib.contains(&format!("include_str!(\"../../../docs/user/{name}\")"))
        {
            missing.push(name.into_owned());
        }
    }
    missing.sort();
    assert!(
        missing.is_empty(),
        "docs/user files not wired into src/lib.rs as doctest modules: {missing:?}"
    );
}

/// Public type-like items whose parent is a module, as `grid::Screen` or `Terminal`.
fn public_types(snapshot: &str) -> Vec<String> {
    let modules: Vec<&str> = snapshot
        .lines()
        .filter_map(|line| line.strip_prefix("pub mod "))
        .collect();
    let mut names = Vec::new();
    for line in snapshot.lines() {
        let mut rest = line;
        while rest.starts_with("#[") {
            match rest.split_once("] ") {
                Some((_, after)) => rest = after,
                None => break,
            }
        }
        let Some(declaration) = ["struct ", "enum ", "trait ", "type ", "union "]
            .iter()
            .find_map(|kind| rest.strip_prefix("pub ")?.strip_prefix(kind))
        else {
            continue;
        };
        let end = declaration
            .find(['<', '(', ' ', ';'])
            .unwrap_or(declaration.len());
        // A trait with a supertrait prints as `pub trait path: Super`.
        let path = declaration[..end].trim_end_matches(':');
        let Some((parent, name)) = path.rsplit_once("::") else {
            continue;
        };
        if modules.contains(&parent) {
            let short = path.strip_prefix("oneterm_vt::").unwrap_or(name);
            names.push(short.to_owned());
        }
    }
    names.sort();
    names.dedup();
    names
}

#[test]
fn api_map_names_every_public_type() {
    let api_map = read(&user_docs_dir().join("api-map.md"));
    let mut missing = Vec::new();
    for snapshot in ["public-api.unix.txt", "public-api.windows.txt"] {
        let types = public_types(&read(&manifest_dir().join(snapshot)));
        assert!(
            types.len() > 50,
            "{snapshot}: parsed only {} types; the format changed?",
            types.len()
        );
        for name in types {
            if !api_map.contains(&format!("`{name}`")) && !missing.contains(&name) {
                missing.push(name);
            }
        }
    }
    assert!(
        missing.is_empty(),
        "types without a row in docs/user/api-map.md: {missing:?}"
    );
}
