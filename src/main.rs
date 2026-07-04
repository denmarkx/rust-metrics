mod writer;
mod analyze;
mod downloader;

use std::sync::Arc;

use tokio::sync::mpsc;
use clap::{arg, command, value_parser};

#[tokio::main]
async fn main() {
    let matches = command!()
        .arg(
            arg!(-n --numdownloads <NUM> "The number of crates to download.")
            .required(false)
            .value_parser(value_parser!(usize))
        )
        .arg(
            arg!(-i --internalcap "Capacity of internal download->analysis buffer before analyzing.")
            .required(false)
            .value_parser(value_parser!(usize))
            .default_value("500")
        )
        .arg(
            arg!(-b --downloadcap "Maximum number of active workers for the download tasks.")
            .required(false)
            .default_value("10")
            .value_parser(value_parser!(usize))
        )
        .arg(
            arg!(-r --readcap "Maximum number of active workers for the analysis read tasks.")
            .required(false)
            .default_value("20")
            .value_parser(value_parser!(usize))
        )
        .arg(
            arg!(-c --writecap "Capacity of internal analysis buffer before writing to file.")
            .required(false)
            .value_parser(value_parser!(usize))
            .default_value("500")
        )
        .get_matches();

    let buffer_size = matches.get_one::<usize>("internalcap").unwrap();
    let (tx, rx) = mpsc::channel::<downloader::Crate>(*buffer_size);
    let tx_arc = Arc::new(tx);

    let num_crates = matches.get_one::<usize>("numdownloads");
    let buffer_cap = matches.get_one::<usize>("downloadcap").unwrap();
    let download = downloader::download(tx_arc, num_crates, buffer_cap);

    let read_buffer_cap = matches.get_one::<usize>("readcap").unwrap();
    let write_buffer_cap = matches.get_one::<usize>("writecap").unwrap();
    let analysis = analyze::analyze(rx, *read_buffer_cap, *write_buffer_cap);

    tokio::join!(download, analysis);
}
