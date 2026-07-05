use crate::error_handling::{handle_error, handle_error_raw};
use async_compression::futures::bufread::GzipDecoder;
use futures_util::stream::TryStreamExt;
use futures::stream::{self, StreamExt};
use futures::io::BufReader;
use futures::io::copy;
use reqwest::Client;
use anyhow::Result;

use std::sync::atomic::{AtomicUsize, Ordering};
use std::io::{Error, ErrorKind};
use std::sync::{Arc, Mutex};
use std::fs::{DirBuilder, read_dir};
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

#[derive(Serialize, Deserialize)]
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

fn get_crates(num_downloads: Option<&usize>) -> Vec<Crate> {
    let registry_path = find_registry();
    println!("Crate Registry Path: {:?}", registry_path);

    let index = GitIndex::with_path(registry_path, CRATE_INDEX_URL)
        .expect("Failed to find or clone Cargo registry.");

    if let Some(n) = num_downloads {
        let crates : Vec<Crate> = index.crates_parallel()
            .take_any(*n)
            .map(|r| {
                let data = r.unwrap();
                Crate { name: data.name().to_string(), version: data.highest_version().version().to_string() }
            })
            .collect();
        return crates;
    }

    let crates : Vec<Crate> = index.crates_parallel()
        .filter_map(|r| {
            let data = r.unwrap();
            if want_crate(&data.name().to_string()) {
                Some(Crate { name: data.name().to_string(), version: data.highest_version().version().to_string() })
            } else {
                None
            }
        })
        .collect();
    return crates;
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
    archive.unpack(CRATE_OUTPUT_DIR).await?;

    // It's not really a damaging error if this fails..
    if let Err(_) = std::fs::remove_file(&output_file_path) {
        handle_error(&c, "remove_crate_file");
    }

    Ok(())
}

pub async fn download(tx: Arc<mpsc::Sender<Crate>>, num_downloads : Option<&usize>, buffer_cap: &usize) {
    let crate_dir_path = Path::new(CRATE_OUTPUT_DIR);
    if !crate_dir_path.exists() {
        DirBuilder::new().create(&crate_dir_path)
            .expect("Failed to create crate output directory.");
    }

    let crates : Vec<Crate> = get_crates(num_downloads);
    let total = crates.len();
    let count = Arc::new(AtomicUsize::new(total));

    let client = Client::new();
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
