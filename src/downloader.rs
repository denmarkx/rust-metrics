use std::collections::HashSet;
use crate::error_handling::handle_error;
use async_compression::futures::bufread::GzipDecoder;
use futures_util::stream::TryStreamExt;
use futures::stream::{self, StreamExt};
use futures::io::BufReader;
use futures::io::copy;
use reqwest::Client;
use anyhow::Result;

use std::sync::atomic::{AtomicUsize, Ordering};
use std::io::{Error, ErrorKind};
use std::fs::DirBuilder;
use std::path::Path;
use std::sync::Arc;

use tokio_util::compat::TokioAsyncWriteCompatExt;
use tokio::sync::mpsc;
use tokio_tar::Archive;
use tokio::fs::File;

use crate::index::{Crate, get_crates};

const CRATE_OUTPUT_DIR: &str = "crates";

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
            println!("{remainder} crates left remaining.");
        }
    })
    .buffer_unordered(*buffer_cap)
    .for_each(|_| async move {})
    .await;
}
