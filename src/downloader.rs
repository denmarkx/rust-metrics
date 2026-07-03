use async_compression::futures::bufread::GzipDecoder;
use futures_util::stream::TryStreamExt;
use futures::stream::{self, StreamExt};
use futures::io::BufReader;
use futures::io::copy;
use reqwest::Client;

use std::io::{Error, ErrorKind};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::fs::DirBuilder;
use std::path::Path;

use tokio_util::compat::TokioAsyncWriteCompatExt;
use tokio_tar::Archive;
use tokio::fs::File;

use rayon::iter::ParallelIterator;
use crates_index::GitIndex;

const CRATE_OUTPUT_DIR: &str = "crates";

struct Crate {
    name: String,
    version: String
}

fn get_crates(num_downloads: Option<&usize>) -> Vec<Crate> {
    let index = GitIndex::new_cargo_default().expect("Failed to find or clone Cargo registry.");

    let mut crates = Vec::new();
    let arc_vec = Arc::new(Mutex::new(&mut crates));

    let it = match num_downloads {
        Some(n) => index.crates_parallel().take_any(*n),
        None => {
            let size = index.crates_parallel().count();
            index.crates_parallel().take_any(size)
        }
    };

    it.for_each(|c| {
        if let Ok(x) = c {
            let cs = Crate {
                name: x.name().to_string(),
                version: x.highest_version().version().to_string()
            };

            let mut vec = arc_vec.lock().unwrap();
            vec.push(cs);
        } else {
            println!("Failed to extract crate data.");
        }
    });
    return crates;
}

pub async fn download(num_downloads: Option<&usize>, buffer_cap: &usize) {
    let crates : Vec<Crate> = get_crates(num_downloads);
    let num_crates = crates.len();
    let targets = stream::iter(crates);

    let client = Client::new();
    let count_arc = Arc::new(AtomicUsize::new(1));
    let _ = targets.map(|c| {
        let client = client.clone();
        let count_clone = count_arc.clone();
        async move {
            let crate_url = format!("https://static.crates.io/crates/{}/{}-{}.crate", c.name, c.name, c.version);
            let resp = client.get(crate_url).send().await;

            let count = count_clone.fetch_add(1, Ordering::SeqCst);
            println!("Downloading Crate [{}/{}]: {}", count, num_crates, c.name);

            let crate_dir_path = Path::new(CRATE_OUTPUT_DIR);
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
    .buffer_unordered(*buffer_cap)
    .for_each(|_| async move {})
    .await;
}
