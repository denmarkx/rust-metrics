mod writer;
mod analyze;
mod downloader;

use clap::{arg, command, value_parser};

#[tokio::main]
async fn main() {
    let matches = command!()
        .arg(
            arg!(-d --download "Clones the Crates registry, downloads, and unzips all crates.")
            .required(false)
        )
        .arg(
            arg!(-n --numdownloads <NUM> "The number of crates to download.")
            .required(false)
            .value_parser(value_parser!(usize))
        )
        .arg(
            arg!(-a --analyze "Analyzes all downloaded crates and writes to .parquet file.")
            .required(false)
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

    if matches.get_flag("download") {
        let num_crates = matches.get_one::<usize>("numdownloads");
        let buffer_cap = matches.get_one::<usize>("downloadcap").unwrap();
        downloader::download(num_crates, buffer_cap).await
    }

    if matches.get_flag("analyze") {
        let read_buffer_cap = matches.get_one::<usize>("readcap").unwrap();
        let write_buffer_cap = matches.get_one::<usize>("writecap").unwrap();
        analyze::analyze(read_buffer_cap, *write_buffer_cap).await
    }

}
