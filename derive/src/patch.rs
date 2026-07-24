extern crate proc_macro;
use proc_macro2::{Ident, Span, TokenStream};
use quote::{quote, ToTokens};
use std::str::FromStr;
use syn::spanned::Spanned;
use syn::{parenthesized, DeriveInput, Lit, LitStr, Result, Type};

#[cfg(feature = "op")]
use crate::Addable;

const PATCH: &str = "patch";
const NAME: &str = "name";
const ATTRIBUTE: &str = "attribute";
const SKIP: &str = "skip";
const ADDABLE: &str = "addable";
const ADD: &str = "add";
const NESTING: &str = "nesting";
const EMPTY_VALUE: &str = "empty_value";
const SKIP_WRAP: &str = "skip_wrap";
/// Container attribute: skip generating `into_patch_by_diff`. Useful for
/// structs whose fields cannot derive `PartialEq` (e.g. they contain
/// `RefCell`, locks, or opaque runtime caches). The trait's default
/// `into_patch_by_diff` then kicks in, which just falls back to
/// `into_patch()`.
const NO_DIFF: &str = "no_diff";
/// Container attribute: emit `#[serde(skip_serializing_if = "Option::is_none")]`
/// on every `Option`-wrapped patch field, so unset fields disappear from the
/// serialized patch instead of showing up as explicit `null`s. Requires the
/// patch struct to derive `serde::Serialize`.
const SKIP_SERIALIZING_NONE: &str = "skip_serializing_none";
/// Field attribute for `Option<T>` fields: keep the double-`Option` patch
/// type (`Option<Option<T>>`) and emit the serde plumbing that makes the
/// three states round-trip through JSON — missing key (`None` = no change),
/// explicit `null` (`Some(None)` = clear the field) and a value
/// (`Some(Some(v))` = set). See `struct_patch::serde_utils::deserialize_some`.
const NULLABLE: &str = "nullable";
#[cfg(feature = "list")]
const LIST_PATCH: &str = "list_patch";
#[cfg(feature = "list")]
const LIST_ID: &str = "id";
#[cfg(feature = "list")]
const LIST_ID_TYPE: &str = "id_type";
#[cfg(feature = "list")]
const LIST_PATCH_TYPE: &str = "patch_type";

pub(crate) struct Patch {
    visibility: syn::Visibility,
    struct_name: Ident,
    patch_struct_name: Ident,
    generics: syn::Generics,
    attributes: Vec<TokenStream>,
    fields: Vec<Field>,
    /// Set by `#[patch(no_diff)]`: suppress generation of the
    /// `into_patch_by_diff` override so the trait's default impl runs,
    /// avoiding the `PartialEq` bound on every field.
    no_diff: bool,
}

enum SpecialAttr {
    None,
    /// Field uses an explicit sentinel value instead of `Option` wrapping.
    EmptyValue(Lit),
    /// Field type is already `Option<T>`; `None` means "no change", `Some(v)` applies the value.
    SkipWrap,
}

impl SpecialAttr {
    fn is_empty(&self) -> bool {
        matches!(self, SpecialAttr::None)
    }

    fn empty_value(&self) -> Option<&Lit> {
        if let SpecialAttr::EmptyValue(lit) = self {
            Some(lit)
        } else {
            None
        }
    }
}

/// Parsed info for a `#[patch(list_patch(...))]` field.
#[cfg(feature = "list")]
struct ListPatchInfo {
    /// The element type `T` extracted from `Vec<T>`.
    element_type: Type,
    /// The id type used for addressing elements.
    id_type: Type,
    /// The closure `|&T| -> ID`.
    id_fn: syn::Expr,
    /// Optional override for the patch type of the element. Defaults to
    /// `{T}Patch`.
    patch_type: Option<Type>,
}

struct Field {
    ident: Option<Ident>,
    ty: Type,
    attributes: Vec<TokenStream>,
    retyped: bool,
    #[cfg(feature = "op")]
    addable: Addable,
    #[cfg(feature = "nesting")]
    nesting: bool,
    special_attr: SpecialAttr,
    #[cfg(feature = "list")]
    list_patch: Option<ListPatchInfo>,
}

impl Field {
    /// Whether this field is a list-patch field. Returns `false` when the
    /// `list` feature is disabled.
    #[cfg(feature = "list")]
    fn is_list_patch(&self) -> bool {
        self.list_patch.is_some()
    }
    #[cfg(not(feature = "list"))]
    fn is_list_patch(&self) -> bool {
        false
    }
}

impl Patch {
    /// Generate the token stream for the patch struct and it resulting implementations
    pub fn to_token_stream(&self) -> Result<TokenStream> {
        let Patch {
            visibility,
            struct_name,
            patch_struct_name: name,
            generics,
            attributes,
            fields,
            no_diff,
        } = self;

        let patch_struct_fields = fields
            .iter()
            .map(|f| f.to_token_stream())
            .collect::<Result<Vec<_>>>()?;

        // Field names
        #[cfg(not(feature = "nesting"))]
        let field_names = fields
            .iter()
            .filter(|f| f.special_attr.is_empty() && !f.is_list_patch())
            .map(|f| f.ident.as_ref())
            .collect::<Vec<_>>();
        #[cfg(not(feature = "nesting"))]
        let field_names_by_empty_value = fields
            .iter()
            .filter(|f| matches!(f.special_attr, SpecialAttr::EmptyValue(_)) && !f.is_list_patch())
            .map(|f| f.ident.as_ref())
            .collect::<Vec<_>>();
        #[cfg(feature = "nesting")]
        let field_names = fields
            .iter()
            .filter(|f| !f.nesting && f.special_attr.is_empty() && !f.is_list_patch())
            .map(|f| f.ident.as_ref())
            .collect::<Vec<_>>();
        #[cfg(feature = "nesting")]
        let field_names_by_empty_value = fields
            .iter()
            .filter(|f| {
                !f.nesting
                    && matches!(f.special_attr, SpecialAttr::EmptyValue(_))
                    && !f.is_list_patch()
            })
            .map(|f| f.ident.as_ref())
            .collect::<Vec<_>>();
        let field_name_empty_values = fields
            .iter()
            .filter(|f| !f.is_list_patch())
            .filter_map(|f| f.special_attr.empty_value())
            .collect::<Vec<_>>();

        // Fields with `#[patch(skip_wrap)]` — the patch keeps the original
        // (already-`Option`) type, and `None` in the patch means "no change".
        #[cfg(not(feature = "nesting"))]
        let skip_wrap_field_names = fields
            .iter()
            .filter(|f| matches!(f.special_attr, SpecialAttr::SkipWrap) && !f.is_list_patch())
            .map(|f| f.ident.as_ref())
            .collect::<Vec<_>>();
        #[cfg(feature = "nesting")]
        let skip_wrap_field_names = fields
            .iter()
            .filter(|f| {
                matches!(f.special_attr, SpecialAttr::SkipWrap) && !f.nesting && !f.is_list_patch()
            })
            .map(|f| f.ident.as_ref())
            .collect::<Vec<_>>();

        // Rename fields
        #[cfg(not(feature = "nesting"))]
        let renamed_field_names = fields
            .iter()
            .filter(|f| f.retyped && f.special_attr.is_empty() && !f.is_list_patch())
            .map(|f| f.ident.as_ref())
            .collect::<Vec<_>>();
        #[cfg(not(feature = "nesting"))]
        let renamed_field_names_by_empty_value = fields
            .iter()
            .filter(|f| {
                f.retyped
                    && matches!(f.special_attr, SpecialAttr::EmptyValue(_))
                    && !f.is_list_patch()
            })
            .map(|f| f.ident.as_ref())
            .collect::<Vec<_>>();
        #[cfg(feature = "nesting")]
        let renamed_field_names = fields
            .iter()
            .filter(|f| f.retyped && !f.nesting && f.special_attr.is_empty() && !f.is_list_patch())
            .map(|f| f.ident.as_ref())
            .collect::<Vec<_>>();
        #[cfg(feature = "nesting")]
        let renamed_field_names_by_empty_value = fields
            .iter()
            .filter(|f| {
                f.retyped
                    && !f.nesting
                    && matches!(f.special_attr, SpecialAttr::EmptyValue(_))
                    && !f.is_list_patch()
            })
            .map(|f| f.ident.as_ref())
            .collect::<Vec<_>>();
        let renamed_field_name_empty_values = fields
            .iter()
            .filter(|f| f.retyped && !f.is_list_patch())
            .filter_map(|f| f.special_attr.empty_value())
            .collect::<Vec<_>>();

        // Original fields
        #[cfg(not(feature = "nesting"))]
        let original_field_names = fields
            .iter()
            .filter(|f| !f.retyped && f.special_attr.is_empty() && !f.is_list_patch())
            .map(|f| f.ident.as_ref())
            .collect::<Vec<_>>();
        #[cfg(not(feature = "nesting"))]
        let original_field_names_by_empty_value = fields
            .iter()
            .filter(|f| {
                !f.retyped
                    && matches!(f.special_attr, SpecialAttr::EmptyValue(_))
                    && !f.is_list_patch()
            })
            .map(|f| f.ident.as_ref())
            .collect::<Vec<_>>();
        #[cfg(feature = "nesting")]
        let original_field_names = fields
            .iter()
            .filter(|f| !f.retyped && !f.nesting && f.special_attr.is_empty() && !f.is_list_patch())
            .map(|f| f.ident.as_ref())
            .collect::<Vec<_>>();
        #[cfg(feature = "nesting")]
        let original_field_names_by_empty_value = fields
            .iter()
            .filter(|f| {
                !f.retyped
                    && !f.nesting
                    && matches!(f.special_attr, SpecialAttr::EmptyValue(_))
                    && !f.is_list_patch()
            })
            .map(|f| f.ident.as_ref())
            .collect::<Vec<_>>();
        #[cfg(not(feature = "nesting"))]
        let original_field_name_empty_values = fields
            .iter()
            .filter(|f| !f.retyped && !f.is_list_patch())
            .filter_map(|f| f.special_attr.empty_value())
            .collect::<Vec<_>>();
        #[cfg(feature = "nesting")]
        let original_field_name_empty_values = fields
            .iter()
            .filter(|f| !f.retyped && !f.nesting && !f.is_list_patch())
            .filter_map(|f| f.special_attr.empty_value())
            .collect::<Vec<_>>();

        // Nesting fields
        #[cfg(not(feature = "nesting"))]
        let nesting_field_names: Vec<String> = Vec::new();
        #[cfg(not(feature = "nesting"))]
        let nesting_field_types: Vec<Type> = Vec::new();

        #[cfg(feature = "nesting")]
        let nesting_field_names = fields
            .iter()
            .filter(|f| f.nesting && !f.is_list_patch())
            .map(|f| f.ident.as_ref())
            .collect::<Vec<_>>();
        #[cfg(feature = "nesting")]
        let nesting_field_types = fields
            .iter()
            .filter(|f| f.nesting && !f.is_list_patch())
            .map(|f| f.ty.clone())
            .collect::<Vec<_>>();

        // List-patch fields
        #[cfg(not(feature = "list"))]
        let list_patch_field_names: Vec<Option<Ident>> = Vec::new();
        #[cfg(not(feature = "list"))]
        let list_patch_id_fns: Vec<syn::Expr> = Vec::new();
        #[cfg(not(feature = "list"))]
        let list_patch_id_types: Vec<Type> = Vec::new();
        #[cfg(not(feature = "list"))]
        let list_patch_element_types: Vec<Type> = Vec::new();
        #[cfg(not(feature = "list"))]
        let list_patch_patch_types: Vec<TokenStream> = Vec::new();

        #[cfg(feature = "list")]
        let list_patch_field_names = fields
            .iter()
            .filter(|f| f.list_patch.is_some())
            .map(|f| f.ident.clone())
            .collect::<Vec<_>>();
        #[cfg(feature = "list")]
        let list_patch_id_fns = fields
            .iter()
            .filter(|f| f.list_patch.is_some())
            .map(|f| f.list_patch.as_ref().unwrap().id_fn.clone())
            .collect::<Vec<_>>();
        #[cfg(feature = "list")]
        let list_patch_id_types = fields
            .iter()
            .filter(|f| f.list_patch.is_some())
            .map(|f| f.list_patch.as_ref().unwrap().id_type.clone())
            .collect::<Vec<_>>();
        #[cfg(feature = "list")]
        let list_patch_element_types = fields
            .iter()
            .filter(|f| f.list_patch.is_some())
            .map(|f| f.list_patch.as_ref().unwrap().element_type.clone())
            .collect::<Vec<_>>();
        #[cfg(feature = "list")]
        let list_patch_patch_types = fields
            .iter()
            .filter(|f| f.list_patch.is_some())
            .map(|f| {
                let info = f.list_patch.as_ref().unwrap();
                match &info.patch_type {
                    Some(t) => t.to_token_stream(),
                    None => {
                        let s = info.element_type.to_token_stream().to_string();
                        let ident =
                            Ident::new(&format!("{}Patch", s.replace(' ', "")), Span::call_site());
                        quote! { #ident }
                    }
                }
            })
            .collect::<Vec<_>>();

        let mapped_attributes = attributes
            .iter()
            .map(|a| {
                quote! {
                    #[#a]
                }
            })
            .collect::<Vec<_>>();

        let patch_struct = quote! {
            #(#mapped_attributes)*
            #visibility struct #name #generics {
                #(#patch_struct_fields)*
            }
        };
        let where_clause = &generics.where_clause;

        #[cfg(feature = "status")]
        let patch_status_impl = quote!(
            #[automatically_derived]
            impl #generics struct_patch::traits::Status for #name #generics #where_clause {
                fn is_empty(&self) -> bool {
                    #(
                        if self.#field_names.is_some() {
                            return false
                        }
                    )*
                    #(
                        if self.#field_names_by_empty_value == #field_name_empty_values {
                            return false
                        }
                    )*
                    #(
                        if self.#skip_wrap_field_names.is_some() {
                            return false
                        }
                    )*
                    #(
                        if !self.#nesting_field_names.is_empty() {
                            return false
                        }
                     )*
                    #(
                        if !self.#list_patch_field_names.is_empty() {
                            return false
                        }
                    )*
                    true
                }
            }
        );
        #[cfg(not(feature = "status"))]
        let patch_status_impl = quote!();

        #[cfg(feature = "merge")]
        let patch_merge_impl = quote!(
            #[automatically_derived]
            impl #generics struct_patch::traits::Merge for #name #generics #where_clause {
                fn merge(self, other: Self) -> Self {
                    Self {
                        #(
                            #renamed_field_names: match (self.#renamed_field_names, other.#renamed_field_names) {
                                (Some(a), Some(b)) => Some(a.merge(b)),
                                (Some(a), None) => Some(a),
                                (None, Some(b)) => Some(b),
                                (None, None) => None,
                            },
                        )*
                        #(
                            #renamed_field_names_by_empty_value: match (self.#renamed_field_names_by_empty_value == #renamed_field_name_empty_values, other.#renamed_field_names_by_empty_value == #renamed_field_name_empty_values) {
                                (false, false) => self.#renamed_field_names_by_empty_value.merge(other.#renamed_field_names_by_empty_value),
                                (false, true) => self.#renamed_field_names_by_empty_value,
                                (true, false) => other.#renamed_field_names_by_empty_value,
                                (true, true) => #renamed_field_name_empty_values,
                            },
                        )*
                        #(
                            #original_field_names: other.#original_field_names.or(self.#original_field_names),
                        )*
                        #(
                            #original_field_names_by_empty_value: match (self.#original_field_names_by_empty_value == #original_field_name_empty_values, other.#original_field_names_by_empty_value == #original_field_name_empty_values) {
                                (false, false) => self.#original_field_names_by_empty_value.merge(other.#original_field_names_by_empty_value),
                                (false, true) => self.#original_field_names_by_empty_value,
                                (true, false) => other.#original_field_names_by_empty_value,
                                (true, true) => #original_field_name_empty_values,
                            },
                        )*
                        #(
                            #skip_wrap_field_names: other.#skip_wrap_field_names.or(self.#skip_wrap_field_names),
                        )*
                        #(
                            #nesting_field_names: other.#nesting_field_names.merge(self.#nesting_field_names),
                        )*
                        #(
                            #list_patch_field_names: {
                                let mut v = self.#list_patch_field_names;
                                v.extend(other.#list_patch_field_names);
                                v
                            },
                        )*
                    }
                }
            }
        );
        #[cfg(not(feature = "merge"))]
        let patch_merge_impl = quote!();

        #[cfg(feature = "op")]
        let addable_handles = fields
            .iter()
            .map(|f| {
                match (&f.addable, matches!(f.special_attr, SpecialAttr::EmptyValue(_))) {
                    (Addable::AddTrait, true) => quote!(
                        a + &b
                    ),
                    (Addable::AddTrait, false) => quote!(
                        Some(a + &b)
                    ),
                    (Addable::AddFn(f), true) => {
                        quote!(
                            #f(a, b)
                        )
                    },
                    (Addable::AddFn(f), false) => {
                        quote!(
                            Some(#f(a, b))
                        )
                    },
                    (Addable::Disable, _) => quote!(
                        panic!("There are conflict patches, please use `#[patch(addable)]` if you want to add these values.")
                    )
                }
            })
            .collect::<Vec<_>>();

        #[cfg(all(feature = "op", not(feature = "merge")))]
        let op_impl = quote! {
            #[automatically_derived]
            impl #generics core::ops::Shl<#name #generics> for #struct_name #generics #where_clause {
                type Output = Self;

                fn shl(mut self, rhs: #name #generics) -> Self {
                    struct_patch::traits::Patch::apply(&mut self, rhs);
                    self
                }
            }

            #[automatically_derived]
            impl #generics core::ops::Add<Self> for #name #generics #where_clause {
                type Output = Self;

                fn add(mut self, rhs: Self) -> Self {
                    Self {
                        #(
                            #renamed_field_names: match (self.#renamed_field_names, rhs.#renamed_field_names) {
                                (Some(a), Some(b)) => {
                                    #addable_handles
                                },
                                (Some(a), None) => Some(a),
                                (None, Some(b)) => Some(b),
                                (None, None) => None,
                            },
                        )*
                        #(
                            #renamed_field_names_by_empty_value: match (self.#renamed_field_names_by_empty_value == #renamed_field_name_empty_values, rhs.#renamed_field_names_by_empty_value == #renamed_field_name_empty_values) {
                                (false, false) => {
                                    let a = self.#renamed_field_names_by_empty_value;
                                    let b = rhs.#renamed_field_names_by_empty_value;
                                    #addable_handles
                                },
                                (false, true) => self.#renamed_field_names_by_empty_value,
                                (true, false) => rhs.#renamed_field_names_by_empty_value,
                                (true, true) => #renamed_field_name_empty_values,
                            },
                        )*
                        #(
                            #original_field_names: match (self.#original_field_names, rhs.#original_field_names) {
                                (Some(a), Some(b)) => {
                                    #addable_handles
                                },
                                (Some(a), None) => Some(a),
                                (None, Some(b)) => Some(b),
                                (None, None) => None,
                            },
                        )*
                        #(
                            #original_field_names_by_empty_value: match (self.#original_field_names_by_empty_value == #original_field_name_empty_values , rhs.#original_field_names_by_empty_value == #original_field_name_empty_values) {
                                (false, false) => {
                                    let a = self.#original_field_names_by_empty_value;
                                    let b = rhs.#original_field_names_by_empty_value;
                                    #addable_handles
                                },
                                (false, true) => self.#original_field_names_by_empty_value,
                                (true, false) => rhs.#original_field_names_by_empty_value,
                                (true, true) => #original_field_name_empty_values,
                            },
                        )*
                        #(
                            #skip_wrap_field_names: match (self.#skip_wrap_field_names, rhs.#skip_wrap_field_names) {
                                (Some(a), Some(b)) => {
                                    #addable_handles
                                },
                                (Some(a), None) => Some(a),
                                (None, Some(b)) => Some(b),
                                (None, None) => None,
                            },
                        )*
                        #(
                            #nesting_field_names: self.#nesting_field_names + rhs.#nesting_field_names,
                        )*
                        #(
                            #list_patch_field_names: {
                                let mut v = self.#list_patch_field_names;
                                v.extend(rhs.#list_patch_field_names);
                                v
                            },
                        )*
                    }
                }
            }
        };

        #[cfg(all(feature = "op", feature = "merge"))]
        let op_impl = quote! {
            #[automatically_derived]
            impl #generics core::ops::Shl<#name #generics> for #struct_name #generics #where_clause {
                type Output = Self;

                fn shl(mut self, rhs: #name #generics) -> Self {
                    struct_patch::traits::Patch::apply(&mut self, rhs);
                    self
                }
            }

            #[automatically_derived]
            impl #generics core::ops::Shl<#name #generics> for #name #generics #where_clause {
                type Output = Self;

                fn shl(mut self, rhs: Self) -> Self {
                    struct_patch::traits::Merge::merge(self, rhs)
                }
            }

            #[automatically_derived]
            impl #generics core::ops::Add<Self> for #name #generics #where_clause {
                type Output = Self;

                fn add(mut self, rhs: Self) -> Self {
                    Self {
                        #(
                            #renamed_field_names: match (self.#renamed_field_names, rhs.#renamed_field_names) {
                                (Some(a), Some(b)) => {
                                    #addable_handles
                                },
                                (Some(a), None) => Some(a),
                                (None, Some(b)) => Some(b),
                                (None, None) => None,
                            },
                        )*
                        #(
                            #renamed_field_names_by_empty_value: match (self.#renamed_field_names_by_empty_value == #renamed_field_name_empty_values, rhs.#renamed_field_names_by_empty_value == #renamed_field_name_empty_values) {
                                (false, false) => {
                                    let a = self.#renamed_field_names_by_empty_value;
                                    let b = rhs.#renamed_field_names_by_empty_value;
                                    #addable_handles
                                },
                                (false, true) => self.#renamed_field_names_by_empty_value,
                                (true, false) => rhs.#renamed_field_names_by_empty_value,
                                (true, true) => #renamed_field_name_empty_values,
                            },
                        )*
                        #(
                            #original_field_names: match (self.#original_field_names, rhs.#original_field_names) {
                                (Some(a), Some(b)) => {
                                    #addable_handles
                                },
                                (Some(a), None) => Some(a),
                                (None, Some(b)) => Some(b),
                                (None, None) => None,
                            },
                        )*
                        #(
                            #original_field_names_by_empty_value: match (self.#original_field_names_by_empty_value == #original_field_name_empty_values , rhs.#original_field_names_by_empty_value == #original_field_name_empty_values) {
                                (false, false) => {
                                    let a = self.#original_field_names_by_empty_value;
                                    let b = rhs.#original_field_names_by_empty_value;
                                    #addable_handles
                                },
                                (false, true) => self.#original_field_names_by_empty_value,
                                (true, false) => rhs.#original_field_names_by_empty_value,
                                (true, true) => #original_field_name_empty_values,
                            },
                        )*
                        #(
                            #skip_wrap_field_names: match (self.#skip_wrap_field_names, rhs.#skip_wrap_field_names) {
                                (Some(a), Some(b)) => {
                                    #addable_handles
                                },
                                (Some(a), None) => Some(a),
                                (None, Some(b)) => Some(b),
                                (None, None) => None,
                            },
                        )*
                        #(
                            #nesting_field_names: self.#nesting_field_names + rhs.#nesting_field_names,
                        )*
                        #(
                            #list_patch_field_names: {
                                let mut v = self.#list_patch_field_names;
                                v.extend(rhs.#list_patch_field_names);
                                v
                            },
                        )*
                    }
                }
            }
        };

        #[cfg(not(feature = "op"))]
        let op_impl = quote!();

        // The `into_patch_by_diff` override requires `Self: PartialEq` on
        // every field (it diffs field-by-field). `#[patch(no_diff)]` opts
        // out: the trait's default impl runs instead, falling back to a
        // full `into_patch()` so types with non-`PartialEq` fields (e.g.
        // `RefCell`, locks, opaque caches) can still derive `Patch`.
        let into_patch_by_diff_impl = if *no_diff {
            quote!()
        } else {
            quote!(
                fn into_patch_by_diff(self, previous_struct: Self) -> #name #generics {
                    #name {
                        #(
                            #renamed_field_names: if self.#renamed_field_names != previous_struct.#renamed_field_names {
                                Some(self.#renamed_field_names.into_patch_by_diff(previous_struct.#renamed_field_names))
                            }
                            else {
                                None
                            },
                        )*
                        #(
                            #renamed_field_names_by_empty_value: if self.#renamed_field_names_by_empty_value != previous_struct.#renamed_field_names_by_empty_value {
                                self.#renamed_field_names_by_empty_value.into_patch_by_diff(previous_struct.#renamed_field_names_by_empty_value)
                            }
                            else {
                                #renamed_field_name_empty_values
                            },
                        )*
                        #(
                            #original_field_names: if self.#original_field_names != previous_struct.#original_field_names {
                                Some(self.#original_field_names)
                            }
                            else {
                                None
                            },
                        )*
                        #(
                            #original_field_names_by_empty_value: if self.#original_field_names_by_empty_value != previous_struct.#original_field_names_by_empty_value {
                                self.#original_field_names_by_empty_value
                            }
                            else {
                                #original_field_name_empty_values
                            },
                        )*
                        #(
                            #skip_wrap_field_names: if self.#skip_wrap_field_names != previous_struct.#skip_wrap_field_names {
                                self.#skip_wrap_field_names
                            }
                            else {
                                None
                            },
                        )*
                        #(
                            #nesting_field_names: self.#nesting_field_names.into_patch_by_diff(previous_struct.#nesting_field_names),
                        )*
                        #(
                            #list_patch_field_names: {
                                let __id_fn: &dyn Fn(&#list_patch_element_types) -> #list_patch_id_types =
                                    &(#list_patch_id_fns);
                                let mut __prev: Vec<#list_patch_element_types> =
                                    previous_struct.#list_patch_field_names.into_iter().collect();
                                let mut __ops: Vec<struct_patch::list::ListPatchOp<#list_patch_element_types, #list_patch_patch_types, #list_patch_id_types>> = Vec::new();
                                for __x in self.#list_patch_field_names.into_iter() {
                                    let __id_x = __id_fn(&__x);
                                    if let Some(__idx) = __prev.iter().position(|__p| __id_fn(__p) == __id_x) {
                                        let __prev_x = __prev.remove(__idx);
                                        __ops.push(struct_patch::list::ListPatchOp::modify(
                                            __id_x,
                                            __x.into_patch_by_diff(__prev_x),
                                        ));
                                    } else {
                                        __ops.push(struct_patch::list::ListPatchOp::append(__x));
                                    }
                                }
                                for __p in __prev.into_iter() {
                                    __ops.push(struct_patch::list::ListPatchOp::delete(__id_fn(&__p)));
                                }
                                __ops
                            },
                        )*
                    }
                }
            )
        };

        let patch_impl = quote! {
            #[automatically_derived]
            impl #generics struct_patch::traits::Patch< #name #generics > for #struct_name #generics #where_clause  {
                fn apply(&mut self, patch: #name #generics) {
                    #(
                        if let Some(v) = patch.#renamed_field_names {
                            self.#renamed_field_names.apply(v);
                        }
                    )*
                    #(
                        if patch.#renamed_field_names_by_empty_value != #renamed_field_name_empty_values {
                            self.#renamed_field_names_by_empty_value.apply(patch.#renamed_field_names_by_empty_value);
                        }
                    )*
                    #(
                        if let Some(v) = patch.#original_field_names {
                            self.#original_field_names = v;
                        }
                    )*
                    #(
                        if patch.#original_field_names_by_empty_value != #original_field_name_empty_values  {
                            self.#original_field_names_by_empty_value = patch.#original_field_names_by_empty_value ;
                        }
                    )*
                    #(
                        if let Some(v) = patch.#skip_wrap_field_names {
                            self.#skip_wrap_field_names = Some(v);
                        }
                    )*
                    #(
                        self.#nesting_field_names.apply(patch.#nesting_field_names);
                    )*
                    #(
                        {
                            let __id_fn: &dyn Fn(&#list_patch_element_types) -> #list_patch_id_types =
                                &(#list_patch_id_fns);
                            struct_patch::list::ListPatchApply::apply_list_patch_ops(
                                &mut self.#list_patch_field_names,
                                patch.#list_patch_field_names,
                                __id_fn,
                            );
                        }
                    )*
                }

                fn into_patch(self) -> #name #generics {
                    #name {
                        #(
                            #renamed_field_names: Some(self.#renamed_field_names.into_patch()),
                        )*
                        #(
                            #renamed_field_names_by_empty_value: self.#renamed_field_names_by_empty_value.into_patch(),
                        )*
                        #(
                            #original_field_names: Some(self.#original_field_names),
                        )*
                        #(
                            #original_field_names_by_empty_value: self.#original_field_names_by_empty_value,
                        )*
                        #(
                            #skip_wrap_field_names: self.#skip_wrap_field_names,
                        )*
                        #(
                            #nesting_field_names: self.#nesting_field_names.into_patch(),
                        )*
                        #(
                            #list_patch_field_names: self.#list_patch_field_names
                                .into_iter()
                                .map(|__v| struct_patch::list::ListPatchOp::append(__v))
                                .collect::<Vec<struct_patch::list::ListPatchOp<#list_patch_element_types, #list_patch_patch_types, #list_patch_id_types>>>(),
                        )*
                    }
                }
                #into_patch_by_diff_impl

                fn new_empty_patch() -> #name #generics {
                    #name {
                        #(
                            #field_names: None,
                        )*
                        #(
                            #field_names_by_empty_value: #field_name_empty_values,
                        )*
                        #(
                            #skip_wrap_field_names: None,
                        )*
                        #(
                            #nesting_field_names: #nesting_field_types::new_empty_patch(),
                        )*
                        #(
                            #list_patch_field_names: Vec::new(),
                        )*
                    }
                }
            }
        };

        Ok(quote! {
            #patch_struct

            #patch_status_impl

            #patch_merge_impl

            #patch_impl

            #op_impl
        })
    }

    /// Parse the patch struct
    pub fn from_ast(
        DeriveInput {
            ident,
            data,
            generics,
            attrs,
            vis,
        }: syn::DeriveInput,
    ) -> Result<Patch> {
        let original_fields = if let syn::Data::Struct(syn::DataStruct { fields, .. }) = data {
            fields
        } else {
            return Err(syn::Error::new(
                ident.span(),
                "Patch derive only use for struct",
            ));
        };

        let mut name = None;
        let mut attributes = vec![];
        let mut fields = vec![];
        let mut no_diff = false;
        let mut skip_serializing_none = false;

        for attr in attrs {
            if attr.path().to_string().as_str() != PATCH {
                continue;
            }

            if let syn::Meta::List(meta) = &attr.meta {
                if meta.tokens.is_empty() {
                    continue;
                }
            }

            attr.parse_nested_meta(|meta| {
                let path = meta.path.to_string();
                match path.as_str() {
                    NAME => {
                        // #[patch(name = "PatchStruct")]
                        if let Some(lit) = crate::get_lit_str(path, &meta)? {
                            if name.is_some() {
                                return Err(meta
                                    .error("The name attribute can't be defined more than once"));
                            }
                            name = Some(lit.parse()?);
                        }
                    }
                    ATTRIBUTE => {
                        // #[patch(attribute(derive(Deserialize)))]
                        // #[patch(attribute(derive(Deserialize, Debug), serde(rename = "foo"))]
                        let content;
                        parenthesized!(content in meta.input);
                        let attribute: TokenStream = content.parse()?;
                        attributes.push(attribute);
                    }
                    NO_DIFF => {
                        // #[patch(no_diff)] — see Patch::no_diff field doc.
                        no_diff = true;
                    }
                    SKIP_SERIALIZING_NONE => {
                        // #[patch(skip_serializing_none)] — see
                        // Patch::skip_serializing_none field doc.
                        skip_serializing_none = true;
                    }
                    _ => {
                        return Err(meta.error(format_args!(
                            "unknown patch container attribute `{}`",
                            path.replace(' ', "")
                        )));
                    }
                }
                Ok(())
            })?;
        }

        for field in original_fields {
            if let Some(f) = Field::from_ast(field, skip_serializing_none)? {
                fields.push(f);
            }
        }

        Ok(Patch {
            visibility: vis,
            patch_struct_name: name.unwrap_or({
                let ts = TokenStream::from_str(&format!("{}Patch", &ident,)).unwrap();
                let lit = LitStr::new(&ts.to_string(), Span::call_site());
                lit.parse()?
            }),
            struct_name: ident,
            generics,
            attributes,
            fields,
            no_diff,
        })
    }
}

impl Field {
    /// Generate the token stream for the Patch struct fields
    pub fn to_token_stream(&self) -> Result<TokenStream> {
        let Field {
            ident,
            ty,
            attributes,
            #[cfg(feature = "nesting")]
            nesting,
            special_attr,
            #[cfg(feature = "list")]
            list_patch,
            ..
        } = self;

        let attributes = attributes
            .iter()
            .map(|a| {
                quote! {
                    #[#a]
                }
            })
            .collect::<Vec<_>>();

        #[cfg(feature = "list")]
        if let Some(info) = list_patch {
            let elem = &info.element_type;
            let id_ty = &info.id_type;
            let patch_ty = match &info.patch_type {
                Some(t) => t.to_token_stream(),
                None => {
                    let s = elem.to_token_stream().to_string();
                    let patch_ident =
                        Ident::new(&format!("{}Patch", s.replace(' ', "")), Span::call_site());
                    quote! { #patch_ident }
                }
            };
            return match ident {
                Some(ident) => Ok(quote! {
                    #(#attributes)*
                    pub #ident: Vec<struct_patch::list::ListPatchOp<#elem, #patch_ty, #id_ty>>,
                }),
                None => Ok(quote! {
                    #(#attributes)*
                    pub Vec<struct_patch::list::ListPatchOp<#elem, #patch_ty, #id_ty>>,
                }),
            };
        }

        match ident {
            #[cfg(not(feature = "nesting"))]
            Some(ident) => {
                if !special_attr.is_empty() {
                    Ok(quote! {
                        #(#attributes)*
                        pub #ident: #ty,
                    })
                } else {
                    Ok(quote! {
                        #(#attributes)*
                        pub #ident: Option<#ty>,
                    })
                }
            }
            #[cfg(feature = "nesting")]
            Some(ident) => {
                if *nesting {
                    // TODO handle rename
                    let patch_type = syn::Ident::new(
                        &format!("{}Patch", &ty.to_token_stream()),
                        Span::call_site(),
                    );
                    Ok(quote! {
                        #(#attributes)*
                        pub #ident: #patch_type,
                    })
                } else if !special_attr.is_empty() {
                    Ok(quote! {
                        #(#attributes)*
                        pub #ident: #ty,
                    })
                } else {
                    Ok(quote! {
                        #(#attributes)*
                        pub #ident: Option<#ty>,
                    })
                }
            }
            #[cfg(not(feature = "nesting"))]
            None => {
                if !special_attr.is_empty() {
                    Ok(quote! {
                        #(#attributes)*
                        pub #ty,
                    })
                } else {
                    Ok(quote! {
                        #(#attributes)*
                        pub Option<#ty>,
                    })
                }
            }
            #[cfg(feature = "nesting")]
            None => {
                if *nesting {
                    // TODO handle rename
                    let patch_type = syn::Ident::new(
                        &format!("{}Patch", &ty.to_token_stream()),
                        Span::call_site(),
                    );
                    Ok(quote! {
                        #(#attributes)*
                        pub #patch_type,
                    })
                } else if !special_attr.is_empty() {
                    Ok(quote! {
                        #(#attributes)*
                        pub #ty,
                    })
                } else {
                    Ok(quote! {
                        #(#attributes)*
                        pub Option<#ty>,
                    })
                }
            }
        }
    }

    /// Parse the patch struct field.
    ///
    /// `skip_serializing_none` is the container-level
    /// `#[patch(skip_serializing_none)]` flag: when set, every `Option`-wrapped
    /// patch field gets `#[serde(skip_serializing_if = "Option::is_none")]`
    /// (fields with their own serialization semantics — `empty_value`,
    /// `skip_wrap`, `nullable`, `nesting`, `list_patch` — are excluded).
    pub fn from_ast(
        syn::Field {
            ident, ty, attrs, ..
        }: syn::Field,
        skip_serializing_none: bool,
    ) -> Result<Option<Field>> {
        let mut attributes = vec![];
        let mut field_type = None;
        let mut skip = false;
        let mut special_attr = SpecialAttr::None;
        let mut nullable = false;

        #[cfg(feature = "op")]
        let mut addable = Addable::Disable;
        #[cfg(feature = "nesting")]
        let mut nesting = false;
        #[cfg(feature = "list")]
        let mut list_patch: Option<ListPatchInfo> = None;

        for attr in attrs {
            if attr.path().to_string().as_str() != PATCH {
                continue;
            }

            if let syn::Meta::List(meta) = &attr.meta {
                if meta.tokens.is_empty() {
                    continue;
                }
            }

            attr.parse_nested_meta(|meta| {
                let path = meta.path.to_string();
                match path.as_str() {
                    SKIP => {
                        // #[patch(skip)]
                        skip = true;
                    }
                    ATTRIBUTE => {
                        // #[patch(attribute(serde(alias = "my-field")))]
                        let content;
                        parenthesized!(content in meta.input);
                        let attribute: TokenStream = content.parse()?;
                        attributes.push(attribute);
                    }
                    NAME => {
                        // #[patch(name = "ItemPatch")]
                        let expr: LitStr = meta.value()?.parse()?;
                        field_type = Some(expr.parse()?)
                    }
                    #[cfg(feature = "op")]
                    ADDABLE => {
                        // #[patch(addable)]
                        addable = Addable::AddTrait;
                    }
                    #[cfg(not(feature = "op"))]
                    ADDABLE => {
                        return Err(syn::Error::new(
                            ident.span(),
                            "`addable` needs `op` feature",
                        ));
                    }
                    #[cfg(feature = "op")]
                    ADD => {
                        // #[patch(add=fn)]
                        let f: Ident = meta.value()?.parse()?;
                        addable = Addable::AddFn(f);
                    }
                    #[cfg(not(feature = "op"))]
                    ADD => {
                        return Err(syn::Error::new(ident.span(), "`add` needs `op` feature"));
                    }
                    #[cfg(feature = "nesting")]
                    NESTING => {
                        // #[patch(nesting)]
                        nesting = true;
                    }
                    #[cfg(not(feature = "nesting"))]
                    NESTING => {
                        return Err(
                            meta.error("#[patch(nesting)] only work with `nesting` feature")
                        );
                    }
                    EMPTY_VALUE => {
                        // #[patch(empty_value = ...)]
                        if matches!(special_attr, SpecialAttr::EmptyValue(_)) {
                            return Err(meta.error(
                                "The empty value is already set, we can't defined more than once",
                            ));
                        }
                        if matches!(special_attr, SpecialAttr::SkipWrap) {
                            return Err(meta.error(
                                "`empty_value` and `skip_wrap` cannot be combined on the same field",
                            ));
                        }
                        if nullable {
                            return Err(meta.error(
                                "`empty_value` and `nullable` cannot be combined on the same field",
                            ));
                        }
                        if let Some(lit) = crate::get_lit(path, &meta)? {
                            special_attr = SpecialAttr::EmptyValue(lit);
                        } else {
                            return Err(meta
                                .error("empty_value needs a clear value to define what is empty"));
                        }
                    }
                    SKIP_WRAP => {
                        // #[patch(skip_wrap)]
                        if matches!(special_attr, SpecialAttr::EmptyValue(_)) {
                            return Err(meta.error(
                                "`skip_wrap` and `empty_value` cannot be combined on the same field",
                            ));
                        }
                        if nullable {
                            return Err(meta.error(
                                "`skip_wrap` and `nullable` cannot be combined on the same field",
                            ));
                        }
                        special_attr = SpecialAttr::SkipWrap;
                    }
                    NULLABLE => {
                        // #[patch(nullable)] — double-Option serde round-trip
                        // for `Option<T>` fields; see the NULLABLE const doc.
                        if matches!(special_attr, SpecialAttr::EmptyValue(_)) {
                            return Err(meta.error(
                                "`nullable` and `empty_value` cannot be combined on the same field",
                            ));
                        }
                        if matches!(special_attr, SpecialAttr::SkipWrap) {
                            return Err(meta.error(
                                "`nullable` and `skip_wrap` cannot be combined on the same field",
                            ));
                        }
                        if !is_option_type(&ty) {
                            return Err(syn::Error::new(
                                ty.span(),
                                "`nullable` requires the field type to be `Option<T>`",
                            ));
                        }
                        nullable = true;
                    }
                    #[cfg(feature = "list")]
                    LIST_PATCH => {
                        // #[patch(list_patch(id = |x| ..., id_type = T, patch_type = ...))]
                        if list_patch.is_some() {
                            return Err(meta.error(
                                "`list_patch` can't be defined more than once on the same field",
                            ));
                        }
                        let content;
                        parenthesized!(content in meta.input);
                        let metas: syn::punctuated::Punctuated<syn::Meta, syn::Token![,]> =
                            syn::punctuated::Punctuated::parse_terminated(&content)?;
                        let mut id_fn: Option<syn::Expr> = None;
                        let mut id_type: Option<Type> = None;
                        let mut patch_type: Option<Type> = None;
                        for m in metas {
                            let syn::Meta::NameValue(nv) = m else {
                                return Err(syn::Error::new(
                                    ident.span(),
                                    "`list_patch` only supports `key = value` items",
                                ));
                            };
                            let m_path = nv.path.to_string();
                            match m_path.as_str() {
                                LIST_ID => {
                                    id_fn = Some(nv.value);
                                }
                                LIST_ID_TYPE => {
                                    let t: Type = syn::parse2(nv.value.to_token_stream())?;
                                    id_type = Some(t);
                                }
                                LIST_PATCH_TYPE => {
                                    let t: Type = syn::parse2(nv.value.to_token_stream())?;
                                    patch_type = Some(t);
                                }
                                _ => {
                                    return Err(syn::Error::new(
                                        nv.path.span(),
                                        format_args!(
                                            "unknown `list_patch` attribute `{}`",
                                            m_path.replace(' ', "")
                                        ),
                                    ));
                                }
                            }
                        }
                        let id_fn = id_fn.ok_or_else(|| {
                            syn::Error::new(
                                ident.span(),
                                "`list_patch` requires `id = <closure>`",
                            )
                        })?;
                        let id_type = id_type.ok_or_else(|| {
                            syn::Error::new(
                                ident.span(),
                                "`list_patch` requires `id_type = <type>`",
                            )
                        })?;
                        let element_type = extract_vec_element_type(&ty).ok_or_else(|| {
                            syn::Error::new(
                                ty.span(),
                                "`list_patch` requires the field to be `Vec<T>`",
                            )
                        })?;
                        list_patch = Some(ListPatchInfo {
                            element_type,
                            id_type,
                            id_fn,
                            patch_type,
                        });
                    }
                    #[cfg(not(feature = "list"))]
                    LIST_PATCH => {
                        return Err(syn::Error::new(
                            ident.span(),
                            "`list_patch` needs the `list` feature",
                        ));
                    }
                    _ => {
                        return Err(meta.error(format_args!(
                            "unknown patch field attribute `{}`",
                            path.replace(' ', "")
                        )));
                    }
                }
                Ok(())
            })?;
            if skip {
                return Ok(None);
            }
        }

        #[cfg(feature = "nesting")]
        let is_nesting = nesting;
        #[cfg(not(feature = "nesting"))]
        let is_nesting = false;
        #[cfg(feature = "list")]
        let is_list_patch = list_patch.is_some();
        #[cfg(not(feature = "list"))]
        let is_list_patch = false;

        if nullable && is_nesting {
            return Err(syn::Error::new(
                ty.span(),
                "`nullable` and `nesting` cannot be combined on the same field",
            ));
        }
        if nullable && is_list_patch {
            return Err(syn::Error::new(
                ty.span(),
                "`nullable` and `list_patch` cannot be combined on the same field",
            ));
        }

        if nullable {
            // Double-Option serde round-trip: `None` (key missing, via
            // `default`) = no change, `Some(None)` (explicit `null`, via
            // `deserialize_some`) = clear, `Some(Some(v))` = set. The skip
            // rule keeps `None` out of the serialized patch entirely, which
            // is what stops "no change" from being misread as "clear".
            attributes.push(quote! {
                serde(
                    default,
                    skip_serializing_if = "Option::is_none",
                    deserialize_with = "struct_patch::serde_utils::deserialize_some"
                )
            });
        } else if skip_serializing_none && special_attr.is_empty() && !is_nesting && !is_list_patch
        {
            attributes.push(quote! {
                serde(skip_serializing_if = "Option::is_none")
            });
        }

        Ok(Some(Field {
            ident,
            retyped: field_type.is_some(),
            ty: field_type.unwrap_or(ty),
            attributes,
            #[cfg(feature = "op")]
            addable,
            #[cfg(feature = "nesting")]
            nesting,
            special_attr,
            #[cfg(feature = "list")]
            list_patch,
        }))
    }
}

trait ToStr {
    fn to_string(&self) -> String;
}

impl ToStr for syn::Path {
    fn to_string(&self) -> String {
        self.to_token_stream().to_string()
    }
}

/// Whether the field type is syntactically `Option<...>`. Used to validate
/// `#[patch(nullable)]`.
fn is_option_type(ty: &Type) -> bool {
    let Type::Path(type_path) = ty else {
        return false;
    };
    type_path
        .path
        .segments
        .last()
        .map_or(false, |seg| seg.ident == "Option")
}

/// Extract the element type `T` from a `Vec<T>` field type. Returns `None`
/// for anything else.
#[cfg(feature = "list")]
fn extract_vec_element_type(ty: &Type) -> Option<Type> {
    let Type::Path(type_path) = ty else {
        return None;
    };
    let last_segment = type_path.path.segments.last()?;
    if last_segment.ident != "Vec" {
        return None;
    }
    let syn::PathArguments::AngleBracketed(args) = &last_segment.arguments else {
        return None;
    };
    if args.args.len() != 1 {
        return None;
    }
    match &args.args[0] {
        syn::GenericArgument::Type(t) => Some(t.clone()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions_sorted::assert_eq_sorted;
    use syn::token::Pub;

    use super::*;

    #[test]
    fn parse_patch() {
        // Test case 1: Valid patch with attributes and fields
        let input = quote! {
            #[derive(Patch)]
            #[patch(name = "MyPatch", attribute(derive(Debug, PartialEq, Clone, Serialize, Deserialize)))]
            pub struct Item {
                #[patch(name = "SubItemPatch")]
                pub field1: SubItem,
                #[patch(skip)]
                pub field2: Option<String>,
                #[patch(empty_value = false)]
                pub field3: bool,
            }
        };
        let expected = Patch {
            visibility: syn::Visibility::Public(Pub::default()),
            struct_name: syn::Ident::new("Item", Span::call_site()),
            patch_struct_name: syn::Ident::new("MyPatch", Span::call_site()),
            generics: syn::Generics::default(),
            attributes: vec![quote! { derive(Debug, PartialEq, Clone, Serialize, Deserialize) }],
            no_diff: false,
            fields: vec![
                Field {
                    ident: Some(syn::Ident::new("field1", Span::call_site())),
                    ty: LitStr::new("SubItemPatch", Span::call_site())
                        .parse()
                        .unwrap(),
                    attributes: vec![],
                    retyped: true,
                    #[cfg(feature = "op")]
                    addable: Addable::Disable,
                    #[cfg(feature = "nesting")]
                    nesting: false,
                    special_attr: SpecialAttr::None,
                    #[cfg(feature = "list")]
                    list_patch: None,
                },
                Field {
                    ident: Some(syn::Ident::new("field3", Span::call_site())),
                    ty: LitStr::new("bool", Span::call_site()).parse().unwrap(),
                    attributes: vec![],
                    retyped: false,
                    #[cfg(feature = "op")]
                    addable: Addable::Disable,
                    #[cfg(feature = "nesting")]
                    nesting: false,
                    special_attr: SpecialAttr::EmptyValue(Lit::Bool(syn::LitBool::new(
                        false,
                        Span::call_site(),
                    ))),
                    #[cfg(feature = "list")]
                    list_patch: None,
                },
            ],
        };
        let result = Patch::from_ast(syn::parse2(input).unwrap()).unwrap();
        assert_eq_sorted!(
            format!("{:?}", result.to_token_stream()),
            format!("{:?}", expected.to_token_stream())
        );
    }
}
