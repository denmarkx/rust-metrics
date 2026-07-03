use async_compression::futures::bufread::GzipDecoder;
use futures_util::stream::TryStreamExt;
use futures::io::BufReader;
use futures::io::copy;
use reqwest::Client;

use std::io::{Error, ErrorKind};
use std::fs::DirBuilder;
use std::path::Path;

use tokio_util::compat::TokioAsyncWriteCompatExt;
use tokio_tar::Archive;
use tokio::fs::File;

pub async fn download(output_dir: &str) {
    let client = Client::new();
    let resp = client.get("https://crates.io/api/v1/crates/A-Mazed/0.1.0/download")
        .send()
        .await;

    let crate_dir_path = Path::new(output_dir);
    let _ = crate_dir_path.join("A-Mazed");

    if !crate_dir_path.exists() {
        DirBuilder::new().create(crate_dir_path).expect("Failed to create crate output directory.");
    }

    let output_file_path = Path::new(crate_dir_path).join("A-Mazed.crate");

    let stream = resp.unwrap()
        .bytes_stream()
        .map_err(|e| Error::new(ErrorKind::Other, e))
        .into_async_read();

    let mut output_file = File::create(&output_file_path).await
        .expect("Failed to create .crate file.")
        .compat_write();

    let buf_reader = BufReader::new(stream);
    let gz_decoder = GzipDecoder::new(buf_reader);
    let _ = copy(gz_decoder, &mut output_file).await;

    let file = File::open(&output_file_path).await;
    let mut archive = Archive::new(file.unwrap());
    let _ = archive.unpack(&crate_dir_path).await;
    let _ = std::fs::remove_file(&output_file_path);
}
