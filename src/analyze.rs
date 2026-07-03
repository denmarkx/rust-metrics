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
use glob::glob;
use std::path::PathBuf;
use std::fs;

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

pub async fn analyze(read_cap: &usize, write_cap: usize) {
    let (tx, mut rx) = mpsc::channel::<CrateData>(write_cap);

    let write_handle = tokio::spawn(async move {
        let mut writer = Writer::new(WRITE_FILE_NAME).await;
        let mut buffer = Vec::with_capacity(write_cap);

        while rx.recv_many(&mut buffer, write_cap).await > 0 {
            writer.write(&buffer).await.unwrap();
            buffer.clear();
        }

        writer.close().await.unwrap();
    });

    let it = glob("crates/*");
    let mut dirs : Vec<PathBuf> = Vec::new();
    it.unwrap().for_each(|x| { dirs.push(x.unwrap() )});

    let targets = stream::iter(dirs);
    let _ = targets.map(|p| {
        let tx_clone = tx.clone();
        async move {
            let path_str = p.to_str().unwrap();
            let pattern = format!("{}/**/*.rs", path_str);

            let mut crate_data = CrateData::default();
            crate_data.set_crate_name(p.file_stem().unwrap().to_str().unwrap());

            println!("Analyzing crate: {}", crate_data.crate_name);
            for entry in glob(&pattern).unwrap() {
                let src = fs::read_to_string(entry.unwrap()).unwrap();
                let syntax = syn::parse_file(&src).unwrap();
                crate_data.visit_file(&syntax);
            }
            tx_clone.send(crate_data).await.unwrap();
        }
    })
    .buffer_unordered(*read_cap)
    .for_each(|_| async {})
    .await;

    drop(tx);
    write_handle.await.unwrap();
}
