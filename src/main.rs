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
        .get_matches();

    if matches.get_flag("download") {
        let num_crates = matches.get_one::<usize>("numdownloads");
        downloader::download(num_crates).await
    }

    if matches.get_flag("analyze") {
        analyze::analyze().await
    }

}
