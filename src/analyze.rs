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
use proc_macro2::TokenTree;

#[derive(Debug)]
pub struct Visitor {
    pub unsafe_traits: i32,
    pub unsafe_exprs: i32,
    pub unsafe_impls: i32,
    pub unsafe_funcs: i32,
    pub unsafe_mods: i32,

    pub ffi_export_funcs: i32,
    pub ffi_import_funcs: i32,
}

impl<'a> Visit<'a> for Visitor {
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
