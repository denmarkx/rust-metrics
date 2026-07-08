mod error_handling;
mod writer;
mod analyze;
mod downloader;
mod index;

use crate::downloader::{
    TopCategory,
    download_all,
    download_by_crates,
    download_by_dependencies,
    download_by_number,
    download_by_top_n,
};
use crate::index::{cache_crates, Crate};

use tracing_subscriber;

use clap::{Arg, ArgAction, arg, command, value_parser};
use std::collections::HashMap;
use std::{fs::File, sync::Arc};
use tokio::sync::mpsc;
use std::panic;

const TOP_N_MATCHES: [&str; 2] = ["downloads", "sizes"];

fn main() {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .thread_stack_size(64 * 1024 * 1024)
        .build()
        .unwrap();

    rt.block_on(async_main());
}

async fn async_main() {
    let _ = tracing_subscriber::fmt::init();

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
            arg!(-c --cache "Caches the relevant parts of the Crates.io index to local disk.")
            .required(false)
        )
        .arg(
            arg!(-d --deps "Instead of analyzing the specified crates, analyze crates with the given dependencies.")
            .required(false)
        )
        .arg(
            arg!(-t --top <CATEGORY>)
            .required(false)
            .value_parser(value_parser!(String))
            .help(
                format!("Species only the top-n crates.\
                    Categorized by setting one of: <{}>.\
                    Must be used in concert with -n <NUM>.", TOP_N_MATCHES.join(", "))
            )
        )
        .arg(
            Arg::new("crates")
            .action(ArgAction::Append)
            .help("Reparses only the crates specified. Separate by space for multiple.")
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

    let top_match = matches.get_one::<String>("top");

    let has_all_flag = matches.get_flag("all");
    let has_errors_flag = matches.get_flag("errors");
    let has_deps_flag = matches.get_flag("deps");

    let download = async move {
        match(download_num_opt, download_crates_opt) {
            (None, None) => {
                // Only two options can be specified if not -n or <crates>: -e or -a
                if has_all_flag {
                    download_all(tx_arc, buffer_cap).await
                } else if has_errors_flag {
                    let crates = get_crates_from_errors();
                    download_by_crates(tx_arc, buffer_cap, crates).await
                } else if top_match.is_some() {
                    panic!("--top must be used with -n.")
                } else {
                    panic!("Either -a, -e, -n, or a list of space-separated crate names must be specified.")
                }
            }
            (None, Some(val_ref)) => {
                let crates = val_ref.into_iter().cloned().collect();
                if has_deps_flag {
                    download_by_dependencies(tx_arc, buffer_cap, crates).await
                } else {
                    download_by_crates(tx_arc, buffer_cap, crates).await
                }
            },
            (Some(n), None) => {
                // An additional option here is --top <category>.
                if let None = top_match {
                    return download_by_number(tx_arc, buffer_cap, n).await;
                }

                let category = match &top_match.unwrap().to_lowercase()[..] {
                    "downloads" => TopCategory::Downloads,
                    // "size" => TopCategory::Size,
                    _ => panic!("Invalid category for --top. See: <{}>", TOP_N_MATCHES.join(", ")),
                };

                download_by_top_n(tx_arc, buffer_cap, category, n).await
            },
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

    error_data.into_iter()
        .map(|x| { x.name })
        .collect::<Vec<String>>()
}
