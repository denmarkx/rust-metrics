use async_compression::futures::bufread::GzipDecoder;
use futures_util::stream::TryStreamExt;
use futures::stream::{self, StreamExt};
use futures::io::BufReader;
use futures::io::copy;
use reqwest::Client;

use std::io::{Error, ErrorKind};
use std::fs::DirBuilder;
use std::path::Path;

use tokio_util::compat::TokioAsyncWriteCompatExt;
use tokio_tar::Archive;
use tokio::fs::File;

// use rayon::iter::ParallelIterator;
use crates_index::GitIndex;

const ASYNC_BUFFER_CAP : usize = 5; 

struct Crate {
    name: String,
    version: String
}

fn get_crates() -> Vec<Crate> {
    let index = GitIndex::new_cargo_default().unwrap();
    let c = index.crate_("byteorder").unwrap();
    let cs = Crate {
        name: c.name().to_string(),
        version: c.highest_version().version().to_string()
    };
    // index.crates_parallel
    return vec![cs];
}

pub async fn download(output_dir: &str) {
    let crates : Vec<Crate> = get_crates();
    let targets = stream::iter(crates);

    let client = Client::new();
    let _ = targets.map(|c| {
        let client = client.clone();
        async move {
            let crate_url = format!("https://static.crates.io/crates/{}/{}-{}.crate", c.name, c.name, c.version);
            let resp = client.get(crate_url).send().await;
            println!("Downloading Crate: {}", c.name);

            let crate_dir_path = Path::new(output_dir);
            let _ = crate_dir_path.join(&c.name);

            if !crate_dir_path.exists() {
                DirBuilder::new().create(crate_dir_path).expect("Failed to create crate output directory.");
            }

            let output_file_path = Path::new(crate_dir_path).join(format!("{}.crate", c.name));

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
    })
    .buffer_unordered(ASYNC_BUFFER_CAP)
    .for_each(|r| async move { dbg!(r); })
    .await;
}
