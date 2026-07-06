use std::collections::HashSet;
use crate::error_handling::handle_error;
use async_compression::futures::bufread::GzipDecoder;
use bitcode::{Decode, Encode};
use futures_util::stream::TryStreamExt;
use futures::stream::{self, StreamExt};
use futures::io::BufReader;
use futures::io::copy;
use reqwest::Client;
use anyhow::Result;

use std::sync::atomic::{AtomicUsize, Ordering};
use std::io::{Error, ErrorKind};
use std::sync::Arc;
use std::fs::{self, DirBuilder, read_dir};
use std::path::{Path, PathBuf};
use std::env;

use tokio_util::compat::TokioAsyncWriteCompatExt;
use tokio::sync::mpsc;
use tokio_tar::Archive;
use tokio::fs::File;

use rayon::iter::ParallelIterator;
use crates_index::GitIndex;
use home::cargo_home;

use serde::{Deserialize, Serialize};

const CRATE_INDEX_URL: &str = "https://github.com/rust-lang/crates.io-index";
const CRATE_OUTPUT_DIR: &str = "crates";
const CRATE_INDEX_CACHE: &str = "crates_index.bitcode";

#[derive(Serialize, Deserialize, Decode, Encode)]
pub(crate) struct Crate {
    pub name: String,
    pub version: String
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
            Some(Crate { name: data.name().to_string(), version: data.highest_version().version().to_string() })
        })
        .collect();

    cache_crates_from_vec(&crates);
    return crates;
}

pub fn cache_crates() {
    get_crates_from_git();
}

pub fn cache_crates_from_vec(crates: &Vec<Crate>) {
    let registry_path = find_registry();
    let index_cache_path = registry_path.join(CRATE_INDEX_CACHE);
    println!("Caching Crates Index to: {:?}", index_cache_path);

    let index_cache_bytes = bitcode::encode(crates);
    if let Err(_e) = fs::write(&index_cache_path, &index_cache_bytes) {
        println!("Failed to write cache of crates.io index to local disk.")
    }
}

fn get_crates() -> Vec<Crate> {
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

    return get_crates_from_git();
}

async fn download_crate(c: &Crate, client: &Client) -> Result<()> {
    println!("Downloading Crate: {}", c.name);

    let crate_url = format!("https://static.crates.io/crates/{}/{}-{}.crate", c.name, c.name, c.version);
    let resp = client.get(crate_url).send().await;

    let output_file_path = Path::new(CRATE_OUTPUT_DIR).join(format!("{}.crate", c.name));

    let stream = resp?
        .bytes_stream()
        .map_err(|e| Error::new(ErrorKind::Other, e))
        .into_async_read();

    let mut output_file = File::create(&output_file_path)
        .await?
        .compat_write();

    let buf_reader = BufReader::new(stream);
    let gz_decoder = GzipDecoder::new(buf_reader);
    copy(gz_decoder, &mut output_file).await?;

    let file = File::open(&output_file_path).await?;
    let mut archive = Archive::new(file);

    // Some crates may have invalid paths (depending on OS) due to testing.
    // fortunately, archive.unpack still unpacks even if it errors.
    let _ = archive.unpack(CRATE_OUTPUT_DIR).await;

    // It's not really a damaging error if this fails..
    if let Err(_) = std::fs::remove_file(&output_file_path) {
        handle_error(&c, "remove_crate_file");
    }

    Ok(())
}

fn create_crates_dir() {
    let crate_dir_path = Path::new(CRATE_OUTPUT_DIR);
    if !crate_dir_path.exists() {
        DirBuilder::new().create(&crate_dir_path)
            .expect("Failed to create crate output directory.");
    }
}

pub async fn download_by_number(tx: Arc<mpsc::Sender<Crate>>, buffer_cap: &usize, num_downloads : &usize) {
    let mut crates = get_crates();
    crates.truncate(*num_downloads);
    download(crates, tx, buffer_cap).await;
}

pub async fn download_by_crates(tx: Arc<mpsc::Sender<Crate>>, buffer_cap: &usize, names : Vec<String>) {
    let mut crates = get_crates();
    let subset : HashSet<_> = names.iter().map(|s| s).collect();
    crates.retain(|x| subset.contains(&x.name));
    download(crates, tx, buffer_cap).await;
}

pub async fn download_all(tx: Arc<mpsc::Sender<Crate>>, buffer_cap: &usize) {
    let crates = get_crates();
    download(crates, tx, buffer_cap).await;
}

async fn download(crates: Vec<Crate>, tx: Arc<mpsc::Sender<Crate>>,  buffer_cap: &usize) {
    create_crates_dir();

    let total = crates.len();
    let count = Arc::new(AtomicUsize::new(total));

    let client = Client::new();

    // Technically, it would've been better to partition this by knowing the length
    // of the index in advance so we didn't have to load it all into memory throughout the entire program.
    let _ = stream::iter(crates).map(|c| {
        let client = client.clone();
        let tx_clone = tx.clone();
        let count = count.clone();

        async move {
            if let Ok(_) = download_crate(&c, &client).await {
                tx_clone.send(c).await.unwrap();
            } else {
                handle_error(&c, "download");
            }

            let remainder = count.fetch_sub(1, Ordering::Relaxed);
            println!("{remainder} crates left remaining.");
        }
    })
    .buffer_unordered(*buffer_cap)
    .for_each(|_| async move {})
    .await;
}
