mod analyze;
mod writer;
mod downloader;

use clap::{arg, command};
use syn::visit::Visit;
use tokio::sync::mpsc;
use glob::glob;
use std::fs;

const WRITE_BUFFER_SIZE : usize = 5;
const WRITE_FILE_NAME: &str = "crate_data.parquet";
const CRATE_OUTPUT_DIR: &str = "crates";

#[tokio::main]
async fn main() {
    let matches = command!()
        .arg(
            arg!(-d --download "x")
            .required(false)
        )
        .arg(
            arg!(-a --analyze "Analyzes all downloaded crates and writes to .parquet file.")
            .required(false)
        )
        .get_matches();

    if matches.get_flag("download") {
        start_download().await;
    }

    if matches.get_flag("analyze") {
        start_analysis().await
    }

}

async fn start_download() {
    downloader::download(CRATE_OUTPUT_DIR).await
}

async fn start_analysis() {
    let (tx, mut rx) = mpsc::channel::<analyze::CrateData>(WRITE_BUFFER_SIZE);

    for crate_name in glob("crates/*").unwrap() {
        let path = crate_name.unwrap();
        let path_str = path.to_str().unwrap();
        let pattern = format!("{}/**/*.rs", path_str);
        
        let mut visitor = analyze::CrateData::default();
        visitor.set_crate_name(path.file_stem().unwrap().to_str().unwrap());

        for entry in glob(&pattern).unwrap() {
            let src = fs::read_to_string(entry.unwrap()).unwrap();
            let syntax = syn::parse_file(&src).unwrap();
            visitor.visit_file(&syntax);
        }
        // dbg!(visitor);
    }

    let write_handle = tokio::spawn(async move {
        let mut writer = writer::Writer::new(WRITE_FILE_NAME).await;
        let mut buffer = Vec::with_capacity(WRITE_BUFFER_SIZE);

        while rx.recv_many(&mut buffer, WRITE_BUFFER_SIZE).await > 0 {
            writer.write(&buffer).await.unwrap();
            buffer.clear();
        }
        writer.close().await.unwrap();
    });

    let other_handle = tokio::spawn(async move {
        let tx_clone = tx.clone();
        for i in 0..10 {
            // tx_clone.send(Data { n: i }).await.unwrap();
        }
    });

    let _ = tokio::join!(write_handle, other_handle);
}
