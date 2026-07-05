mod error_handling;
mod writer;
mod analyze;
mod downloader;

use std::sync::Arc;
use std::panic;
use tokio::sync::mpsc;
use clap::{Arg, ArgAction, arg, command, value_parser};

fn main() {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .thread_stack_size(64 * 1024 * 1024)
        .build()
        .unwrap();

    rt.block_on(async_main());
}

async fn async_main() {
    let matches = command!()
        .arg(
            arg!(-n --numdownloads <NUM> "The number of crates to download.")
            .required(false)
            .value_parser(value_parser!(usize))
        )
        .arg(
            arg!(-i --internalcap <NUM> "Capacity of internal download->analysis buffer before analyzing.")
            .required(false)
            .value_parser(value_parser!(usize))
            .default_value("500")
        )
        .arg(
            arg!(-b --downloadcap <NUM> "Maximum number of active workers for the download tasks.")
            .required(false)
            .default_value("50")
            .value_parser(value_parser!(usize))
        )
        .arg(
            arg!(-r --readcap <NUM> "Maximum number of active workers for the analysis read tasks.")
            .required(false)
            .default_value("75")
            .value_parser(value_parser!(usize))
        )
        .arg(
            arg!(-w --writecap <NUM> "Capacity of internal analysis buffer before writing to file.")
            .required(false)
            .value_parser(value_parser!(usize))
            .default_value("1000")
        )
        .arg( // TODO
            arg!(-e --reparse_errors "Parses all crates from errors.json.")
            .required(false)
        )
        .arg(
            Arg::new("crates")
            .action(ArgAction::Append)
            .help("Reparses only the crates specified. Separate by space for multiple.")
            .required(false)
        )
        .arg(
            arg!(-c --cache "Caches the relevant parts of the Crates.io index to local disk.")
            .required(false)
        )
        .get_matches();

    if matches.get_flag("cache") {
        downloader::cache_crates();
    }

    let buffer_cap = matches.get_one::<usize>("downloadcap").unwrap();
    let buffer_size = matches.get_one::<usize>("internalcap").unwrap();
    let (tx, rx) = mpsc::channel::<downloader::Crate>(*buffer_size);
    let tx_arc = Arc::new(tx);

    let download_num_opt = matches.get_one::<usize>("numdownloads");
    let download_crates_opt = matches.get_many::<String>("crates");

    let download = async move {
        match(download_num_opt, download_crates_opt) {
            (None, None) => downloader::download_all(tx_arc, buffer_cap).await,
            (None, Some(val_ref)) => {
                let crates = val_ref.into_iter().cloned().collect();
                downloader::download_by_crates(tx_arc, buffer_cap, crates).await
            },
            (Some(n), None) => downloader::download_by_number(tx_arc, buffer_cap, n).await,
            (Some(_), Some(_)) => panic!("--numdownloads and --crates cannot be specified together."),
        }
    };

    let read_buffer_cap = matches.get_one::<usize>("readcap").unwrap();
    let write_buffer_cap = matches.get_one::<usize>("writecap").unwrap();
    let analysis = analyze::analyze(rx, *buffer_size, *read_buffer_cap, *write_buffer_cap);

    panic::set_hook(Box::new(|_| {
        error_handling::flush();
    }));

    tokio::join!(download, analysis);
    error_handling::flush();
}
