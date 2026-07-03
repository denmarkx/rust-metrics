mod analyze;
mod writer;

use futures::io::BufReader;
use async_compression::futures::bufread::GzipDecoder;
use futures_util::stream::TryStreamExt;
use syn::visit::Visit;
use tokio_util::compat::TokioAsyncWriteCompatExt;
use tokio::sync::mpsc;
use std::io::{Error, ErrorKind};
use std::collections::HashMap;
use futures::io::copy;
use std::sync::Arc;
use arrow::datatypes::{DataType, Schema, Field};
use tokio::fs::File;
use reqwest::Client;
use tokio_tar::Archive;
use tokio;
use arrow::array::UInt32Array;
use arrow::record_batch::RecordBatch;

use std::fs;
use std::process;
use glob::glob;

const WRITE_BUFFER_SIZE : usize = 5;
const WRITE_FILE_NAME: &str = "crate_data.parquet";

#[tokio::main]
async fn main() {
    // let client = Client::new();
    // let mut map = HashMap::new();
    // map.insert("json", true);

    // let resp = client.get("https://crates.io/api/v1/crates/A-Mazed/0.1.0/download")
    //     .send()
    //     .await;

    // let stream = resp.unwrap()
    //     .bytes_stream()
    //     .map_err(|e| Error::new(ErrorKind::Other, e))
    //     .into_async_read();

    // let mut output_file = File::create("test.crate").await.unwrap().compat_write();
    // let buf_reader = BufReader::new(stream);
    // let gz_decoder = GzipDecoder::new(buf_reader);
    // let _ = copy(gz_decoder, &mut output_file).await;

    // let f = File::open("test.crate").await;
    // let mut archive = Archive::new(f.unwrap());
    // let _ = archive.unpack("./dst").await;

    // let src = fs::read_to_string("src/test.rs").unwrap();
    // let syntax = syn::parse_file(&src).unwrap();

    // let mut visitor = analyze::Visitor::default();
    // visitor.visit_file(&syntax);

    let (tx, mut rx) = mpsc::channel::<analyze::CrateData>(5);

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
