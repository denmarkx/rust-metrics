mod analyze;

use futures::io::BufReader;
use async_compression::futures::bufread::GzipDecoder;
use futures_util::stream::TryStreamExt;
use syn::visit::Visit;
use tokio_util::compat::TokioAsyncWriteCompatExt;
use std::io::{Error, ErrorKind};
use std::collections::HashMap;
use futures::io::copy;
use tokio::fs::File;
use reqwest::Client;
use tokio_tar::Archive;
use tokio;

use std::fs;
use std::process;

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

    let src = fs::read_to_string("src/test.rs").unwrap();
    let syntax = syn::parse_file(&src).unwrap();
    dbg!(&syntax);

    let mut visitor = analyze::Visitor {
        ffi_export_funcs: 0, ffi_import_funcs:0,
        unsafe_mods: 0,
        unsafe_traits: 0, unsafe_impls: 0,
        unsafe_exprs: 0, unsafe_funcs: 0
    };

    visitor.visit_file(&syntax);

    dbg!(visitor);
}
