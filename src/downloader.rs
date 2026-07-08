use crate::error_handling::handle_error;

use reqwest::header::{ACCEPT, HeaderValue};
use reqwest::Client;
use anyhow::Result;
use serde_json::Value;

use std::sync::atomic::{AtomicUsize, Ordering};
use std::io::{Error, ErrorKind};
use std::collections::{HashMap, HashSet};
use std::fs::DirBuilder;
use std::path::Path;
use std::sync::Arc;

use async_compression::tokio::bufread::GzipDecoder;
use tokio_util::io::StreamReader;
use futures_util::stream;
use futures::StreamExt;
use tokio_tar::Archive;
use tokio::sync::mpsc;

use crate::index::{Crate, get_crates};

const CRATE_OUTPUT_DIR: &str = "crates";

#[derive(Debug)]
pub enum TopCategory {
    Downloads,
}

async fn download_crate(c: &Crate, client: &Client) -> Result<()> {
    tracing::info!("Downloading Crate: {}", c.name);

    let crate_url = format!("https://static.crates.io/crates/{}/{}-{}.crate", c.name, c.name, c.version);
    let resp = client.get(crate_url).send().await;

    let b_stream = resp?
        .bytes_stream()
        .map(|r| { r.map_err(|e| Error::new(ErrorKind::Other, e)) });

    let stream_reader = StreamReader::new(b_stream);
    let gz_decoder = GzipDecoder::new(stream_reader);
    let mut archive = Archive::new(gz_decoder);

    // Some crates may have invalid paths (depending on OS) due to testing.
    // fortunately, archive.unpack still unpacks even if it errors.
    let _ = archive.unpack(CRATE_OUTPUT_DIR).await;

    Ok(())
}

fn create_crates_dir() {
    let crate_dir_path = Path::new(CRATE_OUTPUT_DIR);
    if !crate_dir_path.exists() {
        DirBuilder::new().create(&crate_dir_path)
            .expect("Failed to create crate output directory.");
    }
}

async fn get_top_crates(category: TopCategory, num: usize) -> Result<Vec<Crate>> {
    let mut query : HashMap<&str, &str> = HashMap::new();
    let num_str = num.to_string();

    match category {
        TopCategory::Downloads => {
            query.insert("sort", "downloads");
            query.insert("per_page", &num_str);
        }
    }

    let client = Client::builder()
        .user_agent("github.com/denmarkx/rust-metrics")
        .build()?;

    let resp = client.get("https://crates.io/api/v1/crates")
        .header(ACCEPT, HeaderValue::from_static("application/json"))
        .query(&query)
        .send()
        .await?;

    let text = resp.text().await?;
    let parsed: Value = serde_json::from_str(&text)?;
    let crate_objects = &parsed["crates"].as_array();

    if let Some(x) = crate_objects {
        let names : Vec<&str> = x.iter()
            .map(|x| x["name"].as_str().unwrap())
            .collect();

        let mut crates = get_crates();
        crates.retain(|c| names.contains(&c.name.as_str()));
        return Ok(crates);
    }

    Err(Error::new(ErrorKind::Other, "Failed to get top crates by category.").into())
}

pub async fn download_by_top_size(tx: Arc<mpsc::Sender<Crate>>, buffer_cap: &usize, num_downloads : &usize) {
    // I thought this was a category within the crates.io API..
    // ..and its also not available in crates_index either.
    // next option is to probably download the daily db dump and query from there..
    todo!("...");
}

pub async fn download_by_top_n(tx: Arc<mpsc::Sender<Crate>>, buffer_cap: &usize, category: TopCategory, num_downloads : &usize) {
    if let Ok(crates) = get_top_crates(category, *num_downloads).await {
        download(crates, tx, buffer_cap).await;
        return;
    }
    tracing::error!("Failed to query top-n crates by category.");
}

pub async fn download_by_number(tx: Arc<mpsc::Sender<Crate>>, buffer_cap: &usize, num_downloads : &usize) {
    let mut crates = get_crates();
    crates.truncate(*num_downloads);
    download(crates, tx, buffer_cap).await;
}

pub async fn download_by_dependencies(tx: Arc<mpsc::Sender<Crate>>, buffer_cap: &usize, names : Vec<String>) {
    let mut crates = get_crates();
    crates.retain(|x| {
        for dep in &x.deps {
            if names.contains(dep) {
                return true;
            }
        }
        return false;
    });
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

async fn download(mut crates: Vec<Crate>, tx: Arc<mpsc::Sender<Crate>>,  buffer_cap: &usize) {
    create_crates_dir();

    let total = crates.len();
    let count = Arc::new(AtomicUsize::new(total));

    // crates.dep is no longer needed at this point and it doesnt need to be on the heap longer than it has to.
    for krate in &mut crates {
        drop(std::mem::take(&mut krate.deps));
    }

    let client = Client::new();

    // Technically, it would've been better to partition this by knowing the length
    // of the index in advance so we didn't have to load it all into memory throughout the entire program.
    let _ = stream::iter(crates).map(|mut c| {
        let client = client.clone();
        let tx_clone = tx.clone();
        let count = count.clone();

        async move {
            if let Ok(_) = download_crate(&c, &client).await {
                // Prior to sending it over, we clear deps to get the Strings off the heap.
                c.deps.clear();

                tx_clone.send(c).await.unwrap();
            } else {
                handle_error(&c, "download");
            }

            let remainder = count.fetch_sub(1, Ordering::Relaxed);
            tracing::info!("{remainder} crates left remaining.");
        }
    })
    .buffer_unordered(*buffer_cap)
    .for_each(|_| async move {})
    .await;
}
