use serde::{Deserialize, Serialize};
use rayon::iter::ParallelIterator;
use bitcode::{Decode, Encode};
use std::fs::{self, read_dir};
use crates_index::GitIndex;
use std::path::PathBuf;
use home::cargo_home;
use std::env;

const CRATE_INDEX_URL: &str = "https://github.com/rust-lang/crates.io-index";
const CRATE_INDEX_CACHE: &str = "crates_index.bitcode";

#[derive(Serialize, Deserialize, Decode, Encode)]
pub struct Crate {
    pub name: String,
    pub version: String,
    pub deps: Vec<String>,
}

/*
 * Attempts to locate the cargo registry: first by CARGO_REGISTRY,
 * then by CARGO_HOME/registry/src (which it'll use the last modified path)
 * and finally locally within the "registry" folder.
 * 
 * If all else fails, crates_index::with_path will clone the index either at
 * the CARGO_REGISTRY env var or within the current directory as registry/.
*/
fn find_registry() -> PathBuf {
    if let Ok(p) = env::var("CARGO_REGISTRY") {
        return PathBuf::from(p)
    }

    if let Ok(p) = cargo_home() {
        let mut registry_path = PathBuf::new();
        registry_path.push(p);
        registry_path.push("registry/index");
        if registry_path.exists() {
            let mut entries: Vec<_> = read_dir(&registry_path)
                .unwrap()
                .filter_map(|e| e.ok())
                .filter(|f| f.file_type().is_ok_and(|t| t.is_dir()))
                .collect();

            if entries.is_empty() {
                return PathBuf::from("registry");
            }

            entries.sort_by_key(|e| {
                e.metadata()
                    .and_then(|m| m.modified())
                    .unwrap_or_else(|_| std::time::SystemTime::UNIX_EPOCH)
            });

            let entry = entries.last().unwrap();
            return entry.path();
        }
    }

    return PathBuf::from("registry");
}

fn get_crates_from_git() -> Vec<Crate> {
    let registry_path = find_registry();
    println!("Crate Registry Path: {:?}", registry_path);

    let index = GitIndex::with_path(&registry_path, CRATE_INDEX_URL)
        .expect("Failed to find or clone Cargo registry.");

    let crates : Vec<Crate> = index.crates_parallel()
        .filter_map(|r| {
            let data = r.unwrap();
            Some(Crate {
                name: data.name().to_string(),
                version: data.highest_version().version().to_string(),
                deps: data.highest_version().dependencies()
                    .iter()
                    .map(|c| c.name().to_string() )
                    .collect::<Vec<String>>(),
            })
        })
        .collect();

    cache_crates_from_vec(&crates);
    return crates;
}

fn cache_crates_from_vec(crates: &Vec<Crate>) {
    let registry_path = find_registry();
    let index_cache_path = registry_path.join(CRATE_INDEX_CACHE);
    println!("Caching Crates Index to: {:?}", index_cache_path);

    let index_cache_bytes = bitcode::encode(crates);
    if let Err(_e) = fs::write(&index_cache_path, &index_cache_bytes) {
        println!("Failed to write cache of crates.io index to local disk.")
    }
}

pub fn get_crates() -> Vec<Crate> {
    let registry_path = find_registry();
    println!("Crate Registry Path: {:?}", registry_path);

    // Check if index cache exists:
    let index_cache_path = registry_path.join(CRATE_INDEX_CACHE);
    if index_cache_path.exists() {
        let index_cache_bytes = fs::read(&index_cache_path);
        if let Ok(x) = index_cache_bytes {
            if let Ok(v) = bitcode::decode(&x) {
                return v;
            }
        }
    }

    println!("Failed to decode Crate Registry. Recloning...");
    return get_crates_from_git();
}

pub fn cache_crates() {
    get_crates_from_git();
}
