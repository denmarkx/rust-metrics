use crate::error_handling::{handle_error_raw, handle_error_raw_name};
use crate::writer::Writer;
use syn::visit::Visit;
use syn::{
    ExprUnsafe,
    ForeignItemFn,
    ImplItemFn,
    ItemImpl,
    ItemTrait,
    ItemMod,
    ItemFn,
    ForeignItem,
};

use futures::stream::{self, StreamExt};
use serde::{Serialize, Deserialize};
use proc_macro2::TokenTree;
use tokio::sync::mpsc;
use crate::downloader;
use anyhow::{Result, bail};
use tokio::fs;
use glob::glob;

const WRITE_FILE_NAME: &str = "crate_data.parquet";

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct CrateData {
    crate_name: String,

    unsafe_traits: u32,
    unsafe_exprs: u32,
    unsafe_impls: u32,
    unsafe_funcs: u32,
    unsafe_mods: u32,

    ffi_export_funcs: u32,
    ffi_import_funcs: u32,
}

impl<'a> CrateData {
    pub fn set_crate_name(&mut self, name: &str) {
        self.crate_name = name.to_string();
    }
}

impl<'a> Visit<'a> for CrateData {
    fn visit_item_impl(&mut self, node: &'a ItemImpl) {
        if node.unsafety.is_some() { self.unsafe_impls += 1; }
        syn::visit::visit_item_impl(self, node);
    }

    fn visit_item_fn(&mut self, node: &'a ItemFn) {
        if node.sig.unsafety.is_some() { self.unsafe_funcs += 1; }
        if node.sig.abi.is_some() { self.ffi_export_funcs += 1; }

        syn::visit::visit_item_fn(self, node);
    }

    fn visit_impl_item_fn(&mut self, node: &'a ImplItemFn) {
        if node.sig.unsafety.is_some() { self.unsafe_funcs += 1; }
        if node.sig.abi.is_some() { self.ffi_export_funcs += 1; }
        syn::visit::visit_impl_item_fn(self, node);
    }

    fn visit_foreign_item_fn(&mut self, node: &'a ForeignItemFn) {
        self.ffi_import_funcs += 1;

        // regardless if they are marked unsafe or not,
        // we consider FFI exported functions to be unsafe.
        self.unsafe_funcs += 1;
        syn::visit::visit_foreign_item_fn(self, node);
    }

    fn visit_foreign_item(&mut self, node: &'a ForeignItem) {
        // syn doesn't parse the "safe" keyword:
        if let ForeignItem::Verbatim(x) = node {
            for token in x.clone().into_iter() {
                if let TokenTree::Ident(ident) = token {
                    if ident.to_string().contains("fn") {
                        self.ffi_import_funcs += 1;

                        // in the sense of safety and FFI boundaries,
                        // i still consider this unsafe.
                        self.unsafe_funcs += 1;
                    }
                }
            }
        }
        syn::visit::visit_foreign_item(self, node);
    }

    fn visit_item_mod(&mut self, node: &'a ItemMod) {
        if node.unsafety.is_some() { self.unsafe_mods += 1; }
        syn::visit::visit_item_mod(self, node);
    }

    fn visit_item_trait(&mut self, node: &'a ItemTrait) {
        if node.unsafety.is_some() { self.unsafe_traits += 1; }
        syn::visit::visit_item_trait(self, node);
    }

    fn visit_expr_unsafe(&mut self, node: &'a ExprUnsafe) {
        self.unsafe_exprs += 1;
        syn::visit::visit_expr_unsafe(self, node);
    }
}

async fn process_crate(data: &mut CrateData, c: &downloader::Crate) -> Result<()> {
    let crate_dir_path = format!("crates/{}-{}", c.name, c.version);
    let pattern = format!("{}/**/*.rs", crate_dir_path);
    let paths = glob(&pattern)?;
    let mut num = 0;

    for entry in paths {
        let src = fs::read_to_string(&entry?).await?;
        let syntax = syn::parse_file(&src)?;
        data.visit_file(&syntax);
        num += 1;
    }

    // Sort of a downloading or pathing error if we found something with nothing in it..
    if num == 0 {
        bail!("No entries found for crate: {}", c.name);
    }

    let result = fs::remove_dir_all(&crate_dir_path).await;
    if let Err(_) = result {
        println!("Failed to remove crate directory: {}.", crate_dir_path)
    }

    Ok(())
}

async fn analyze_stream(chunk: Vec<downloader::Crate>, tx: &mpsc::Sender<CrateData>, read_cap: usize) {
    let targets = stream::iter(chunk);
    let _ = targets.map(|c| {
        let tx_clone = tx.clone();
        async move {
            let mut crate_data = CrateData::default();
            crate_data.set_crate_name(&c.name);
            println!("Analyzing crate: {}", crate_data.crate_name);

            if let Ok(_) = process_crate(&mut crate_data, &c).await {
                if let Ok(_) = tx_clone.send(crate_data).await {
                    return;
                }
            }

            handle_error_raw(c.name, c.version, "analyze_stream");
        }
    })
    .buffer_unordered(read_cap)
    .for_each(|_| async {})
    .await;
}

pub async fn analyze(mut download_rx: mpsc::Receiver<downloader::Crate>, read_cap: usize, write_cap: usize) {
    let (tx, mut rx) = mpsc::channel::<CrateData>(write_cap);

    let write_handle = tokio::spawn(async move {
        let mut writer = Writer::new(WRITE_FILE_NAME).await;
        let mut buffer = Vec::with_capacity(write_cap);

        while rx.recv_many(&mut buffer, write_cap).await > 0 {
            if let Err(_) = writer.write(&buffer).await {
                buffer.iter().for_each(|c| handle_error_raw_name(c.crate_name.clone(), "analyze_writer"));
            }
            buffer.clear();
        }

        writer.close().await.unwrap();
    });

    let download_handle = tokio::spawn(async move {
        let mut buffer = Vec::with_capacity(5);
        while download_rx.recv_many(&mut buffer, 5).await > 0 {
            let chunk : Vec<downloader::Crate> = buffer.drain(..).collect();
            analyze_stream(chunk, &tx, read_cap).await;
        }
    });

    download_handle.await.unwrap();
    write_handle.await.unwrap();
}
