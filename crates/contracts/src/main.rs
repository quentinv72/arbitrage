use std::collections::HashSet;
use std::fs;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::Path;

use convert_case::{Case, Casing};
use ethers::contract::Abigen;
use walkdir::WalkDir;

// Generate Rust bindings for contracts
fn main() {
    // Directory path
    let dir_path = "crates/contracts/src/json_abi";
    let rust_extension = ".rs";
    let mut lib = OpenOptions::new()
        .write(true)
        .truncate(true)
        .open("crates/contracts/src/lib.rs")
        .expect("cannot open file");

    let mut seen: HashSet<String> = HashSet::new();

    // Iterate over all entries in the directory
    for entry in WalkDir::new(dir_path) {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.is_file() && path.extension().unwrap().to_str().unwrap() == "json" {
            let contract_name = extract_contract_name(path);
            if seen.contains(contract_name) {
                panic!("{contract_name} seems duplicated")
            }
            seen.insert(contract_name.to_string());
            let abi = fs::read_to_string(path).unwrap();
            let module_name = contract_name.to_case(Case::Snake);
            let binding_filename = format!(
                "{}{}{}",
                "crates/contracts/src/", module_name, rust_extension
            );
            Abigen::new(contract_name, abi)
                .unwrap()
                .generate()
                .unwrap()
                .write_to_file(binding_filename)
                .unwrap();
            lib.write_all(format!("pub mod {};\n", module_name).as_bytes())
                .unwrap();
        }
    }
}

fn extract_contract_name(path: &Path) -> &str {
    let json_len = ".json".len();
    let filename = path.file_name().unwrap().to_str().unwrap();
    &filename[..filename.len() - json_len]
}
