mod error_handling;
mod writer;
mod analyze;
mod downloader;
mod index;

use crate::downloader::{download_all, download_by_crates, download_by_number};
use crate::index::{cache_crates, Crate};

use clap::{Arg, ArgAction, arg, command, value_parser};
use std::collections::HashMap;
use std::{fs::File, sync::Arc};
use tokio::sync::mpsc;
use regex::Regex;
use std::panic;

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
            arg!(-a --all "Downloads and analyzes all crates.")
            .required(false)
        )
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
        .arg(
            arg!(-e --errors "Parses all crates from errors.json.")
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
        cache_crates();
    }

    let buffer_cap = matches.get_one::<usize>("downloadcap").unwrap();
    let buffer_size = matches.get_one::<usize>("internalcap").unwrap();
    let (tx, rx) = mpsc::channel::<Crate>(*buffer_size);
    let tx_arc = Arc::new(tx);

    let download_num_opt = matches.get_one::<usize>("numdownloads");
    let download_crates_opt = matches.get_many::<String>("crates");

    let has_all_flag = matches.get_flag("all");
    let has_errors_flag = matches.get_flag("errors");

    let download = async move {
        match(download_num_opt, download_crates_opt) {
            (None, None) => {
                // Only two options can be specified if not -n or <crates>: -e or -a
                if has_all_flag {
                    download_all(tx_arc, buffer_cap).await
                } else if has_errors_flag {
                    let crates = get_crates_from_errors();
                    download_by_crates(tx_arc, buffer_cap, crates).await
                } else {
                    panic!("Either -a, -e, -n, or a list of space-separated crate names must be specified.")
                }
            }
            (None, Some(val_ref)) => {
                let crates = val_ref.into_iter().cloned().collect();
                download_by_crates(tx_arc, buffer_cap, crates).await
            },
            (Some(n), None) => download_by_number(tx_arc, buffer_cap, n).await,
            (Some(_), Some(_)) => panic!("--numdownloads and --crates cannot be specified together."),
        }
    };

    let read_buffer_cap = matches.get_one::<usize>("readcap").unwrap();
    let write_buffer_cap = matches.get_one::<usize>("writecap").unwrap();
    let analysis = analyze::analyze(rx, *buffer_size, *read_buffer_cap, *write_buffer_cap);

    tokio::join!(download, analysis);
    error_handling::flush();
}

fn get_crates_from_errors() -> Vec<String> {
    let file = File::open("errors.json")
        .expect("Unable to find or open errors.json.");
    let mut data : HashMap<String, Vec<error_handling::ErrorData>> = serde_json::from_reader(file)
        .expect("Unable to parse errors.json.");
    let error_data = data.remove("crates").unwrap();
    let rgx = Regex::new(r"-\d+\.").unwrap();

    // The error data, by mistake, actually concats the crate name and version together.
    let name_only = |name: &mut String| {
        // Some are actually fine.
        if let Some(m) = rgx.find(name) {
            name.truncate(m.start());
        }
    };

    error_data.into_iter()
        .map(|mut x| { 
            name_only(&mut x.name);
            x.name
        })
        .collect::<Vec<String>>()
}
