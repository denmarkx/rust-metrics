mod writer;
mod analyze;
mod downloader;

use clap::{arg, command};

#[tokio::main]
async fn main() {
    let matches = command!()
        .arg(
            arg!(-d --download "x")
            .required(false)
        )
        .arg(
            arg!(-a --analyze "Analyzes all downloaded crates and writes to .parquet file.")
            .required(false)
        )
        .get_matches();

    if matches.get_flag("download") {
        downloader::download().await
    }

    if matches.get_flag("analyze") {
        analyze::analyze().await
    }

}
