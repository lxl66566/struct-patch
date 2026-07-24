//! This crate provides the [`Patch`] and [`Filler`] traits and accompanying derive macro.
//!
//! Deriving [`Patch`] on a struct will generate a struct similar to the original one, but with all fields wrapped in an `Option`.
//! An instance of such a patch struct can be applied onto the original struct, replacing values only if they are set to `Some`, leaving them unchanged otherwise.
//!
//! The following code shows how `struct-patch` can be used together with `serde` to patch structs with JSON objects.
//! ```rust
//! use struct_patch::Patch;
//! use serde::{Deserialize, Serialize};
//!
//! #[derive(Default, Debug, PartialEq, Patch)]
//! #[patch(attribute(derive(Debug, Default, Deserialize, Serialize)))]
//! struct Item {
//!     field_bool: bool,
//!     field_int: usize,
//!     field_string: String,
//! }
//!
//! fn patch_json() {
//!     let mut item = Item {
//!         field_bool: true,
//!         field_int: 42,
//!         field_string: String::from("hello"),
//!     };
//!
//!     let data = r#"{
//!         "field_int": 7
//!     }"#;
//!
//!     let patch: ItemPatch = serde_json::from_str(data).unwrap();
//!
//!     item.apply(patch);
//!
//!     assert_eq!(
//!         item,
//!         Item {
//!             field_bool: true,
//!             field_int: 7,
//!             field_string: String::from("hello")
//!         }
//!     );
//! }
//! ```
//!
//! More details on how to use the the derive macro, including what attributes are available, are
//! available under [`Patch`]
//!
//! Deriving [`Filler`] on a struct will generate a struct similar to the original one with the
//! field with `Option`, `BTreeMap`, `BTreeSet`, `BinaryHeap`,`HashMap`, `HashSet`, `LinkedList`,
//! `VecDeque `or `Vec`.
//! Any struct implement `Default`, `Extend`, `IntoIterator`, `is_empty` can be used with
//! `#[filler(extenable)]`.
//! Unlike [`Patch`], the [`Filler`] only work on the empty fields of instance.
//!
//! ```rust
//! use struct_patch::Filler;
//!
//! #[derive(Filler)]
//! struct Item {
//!     field_int: usize,
//!     maybe_field_int: Option<usize>,
//! }
//! let mut item = Item {
//!     field_int: 0,
//!     maybe_field_int: None,
//! };
//!
//! let filler_1 = ItemFiller{ maybe_field_int: Some(7), };
//! item.apply(filler_1);
//! assert_eq!(item.maybe_field_int, Some(7));
//!
//! let filler_2 = ItemFiller{ maybe_field_int: Some(100), };
//! item.apply(filler_2);
//! assert_eq!(item.maybe_field_int, Some(7));
//! ```
#![no_std]

#[cfg(feature = "alloc")]
extern crate alloc;

// The `ts` feature derives `ts_rs::TS`, whose generated code names `std::`
// paths. Link `std` while keeping the crate itself `no_std`.
#[cfg(feature = "ts")]
extern crate std;

#[cfg(feature = "catalyst")]
#[doc(hidden)]
pub use struct_patch_derive::Catalyst;
#[doc(hidden)]
pub use struct_patch_derive::Filler;
#[doc(hidden)]
pub use struct_patch_derive::Patch;
#[cfg(feature = "catalyst")]
#[doc(hidden)]
pub use struct_patch_derive::Substrate;
pub mod r#box;
#[cfg(feature = "list")]
pub mod list;
pub mod option;
#[cfg(feature = "serde")]
pub mod serde_utils;
pub mod traits;
pub use traits::*;

#[cfg(all(test, feature = "list", feature = "serde"))]
mod list_tests;

#[cfg(test)]
mod tests {
    extern crate alloc;
    use alloc::string::String;
    use serde::Deserialize;
    #[cfg(feature = "merge")]
    use struct_patch::Merge;
    use struct_patch::Patch;
    #[cfg(feature = "status")]
    use struct_patch::Status;

    use crate as struct_patch;

    #[test]
    fn test_basic() {
        #[derive(Patch, Debug, PartialEq)]
        struct Item {
            field: u32,
            other: String,
        }

        let mut item = Item {
            field: 1,
            other: String::from("hello"),
        };
        let patch = ItemPatch {
            field: None,
            other: Some(String::from("bye")),
        };

        item.apply(patch);
        assert_eq!(
            item,
            Item {
                field: 1,
                other: String::from("bye")
            }
        );
    }

    #[test]
    #[cfg(feature = "status")]
    fn test_empty() {
        #[derive(Patch)]
        #[patch(attribute(derive(Debug, PartialEq)))]
        struct Item {
            data: u32,
        }

        let patch = ItemPatch { data: None };
        let other_patch = Item::new_empty_patch();
        assert!(patch.is_empty());
        assert_eq!(patch, other_patch);
        let patch = ItemPatch { data: Some(0) };
        assert!(!patch.is_empty());
    }

    #[test]
    fn test_derive() {
        #[allow(dead_code)]
        #[derive(Patch)]
        #[patch(attribute(derive(Copy, Clone, PartialEq, Debug)))]
        struct Item;

        let patch = ItemPatch {};
        let other_patch = patch;
        assert_eq!(patch, other_patch);
    }

    #[test]
    fn test_name() {
        #[derive(Patch)]
        #[patch(name = "PatchItem")]
        struct Item;

        let patch = PatchItem {};
        let mut item = Item;
        item.apply(patch);
    }

    #[test]
    fn test_nullable() {
        #[derive(Patch, Debug, PartialEq)]
        struct Item {
            field: Option<u32>,
            other: Option<String>,
        }

        let mut item = Item {
            field: Some(1),
            other: Some(String::from("hello")),
        };
        let patch = ItemPatch {
            field: None,
            other: Some(None),
        };

        item.apply(patch);
        assert_eq!(
            item,
            Item {
                field: Some(1),
                other: None
            }
        );
    }

    #[test]
    fn test_skip() {
        #[derive(Patch, PartialEq, Debug)]
        #[patch(attribute(derive(PartialEq, Debug, Deserialize)))]
        struct Item {
            #[patch(skip)]
            id: u32,
            data: u32,
        }

        let mut item = Item { id: 1, data: 2 };
        let data = r#"{ "id": 10, "data": 15 }"#; // Note: serde ignores unknown fields by default.
        let patch: ItemPatch = serde_json::from_str(data).unwrap();
        assert_eq!(patch, ItemPatch { data: Some(15) });

        item.apply(patch);
        assert_eq!(item, Item { id: 1, data: 15 });
    }

    #[test]
    fn test_nested() {
        #[derive(PartialEq, Debug, Default, Patch, Deserialize)]
        #[patch(attribute(derive(PartialEq, Debug, Deserialize)))]
        struct B {
            c: u32,
            d: u32,
        }

        #[derive(PartialEq, Debug, Patch, Deserialize)]
        #[patch(attribute(derive(PartialEq, Debug, Deserialize)))]
        struct A {
            #[patch(name = "BPatch")]
            b: B,
        }
        let mut b = B::default();
        let b_patch: BPatch = serde_json::from_str(r#"{ "d": 1 }"#).unwrap();
        b.apply(b_patch);
        assert_eq!(b, B { c: 0, d: 1 });

        let mut a = A { b };
        let data = r#"{ "b": { "c": 1 } }"#;
        let patch: APatch = serde_json::from_str(data).unwrap();
        // assert_eq!(
        //     patch,
        //     APatch {
        //         b: Some(B { id: 1 })
        //     }
        // );
        a.apply(patch);
        assert_eq!(
            a,
            A {
                b: B { c: 1, d: 1 }
            }
        );
    }

    #[test]
    fn test_generic() {
        #[derive(Patch)]
        struct Item<T>
        where
            T: PartialEq,
        {
            pub field: T,
        }

        let patch = ItemPatch {
            field: Some(String::from("hello")),
        };
        let mut item = Item {
            field: String::new(),
        };
        item.apply(patch);
        assert_eq!(item.field, "hello");
    }

    #[test]
    fn test_named_generic() {
        #[derive(Patch)]
        #[patch(name = "PatchItem")]
        struct Item<T>
        where
            T: PartialEq,
        {
            pub field: T,
        }

        let patch = PatchItem {
            field: Some(String::from("hello")),
        };
        let mut item = Item {
            field: String::new(),
        };
        item.apply(patch);
    }

    #[test]
    fn test_nested_generic() {
        #[derive(PartialEq, Debug, Default, Patch, Deserialize)]
        #[patch(attribute(derive(PartialEq, Debug, Deserialize)))]
        struct B<T>
        where
            T: PartialEq,
        {
            c: T,
            d: T,
        }

        #[derive(PartialEq, Debug, Patch, Deserialize)]
        #[patch(attribute(derive(PartialEq, Debug, Deserialize)))]
        struct A {
            #[patch(name = "BPatch<u32>")]
            b: B<u32>,
        }

        let mut b = B::default();
        let b_patch: BPatch<u32> = serde_json::from_str(r#"{ "d": 1 }"#).unwrap();
        b.apply(b_patch);
        assert_eq!(b, B { c: 0, d: 1 });

        let mut a = A { b };
        let data = r#"{ "b": { "c": 1 } }"#;
        let patch: APatch = serde_json::from_str(data).unwrap();

        a.apply(patch);
        assert_eq!(
            a,
            A {
                b: B { c: 1, d: 1 }
            }
        );
    }

    #[cfg(feature = "op")]
    #[test]
    fn test_shl() {
        #[derive(Patch, Debug, PartialEq)]
        struct Item {
            field: u32,
            other: String,
        }

        let item = Item {
            field: 1,
            other: String::from("hello"),
        };
        let patch = ItemPatch {
            field: None,
            other: Some(String::from("bye")),
        };

        assert_eq!(
            item << patch,
            Item {
                field: 1,
                other: String::from("bye")
            }
        );
    }

    #[cfg(all(feature = "op", feature = "merge"))]
    #[test]
    fn test_shl_on_patch() {
        #[derive(Patch, Debug, PartialEq)]
        struct Item {
            field: u32,
            other: String,
        }

        let mut item = Item {
            field: 1,
            other: String::from("hello"),
        };
        let patch = ItemPatch {
            field: None,
            other: Some(String::from("bye")),
        };
        let patch2 = ItemPatch {
            field: Some(2),
            other: None,
        };

        let new_patch = patch << patch2;

        item.apply(new_patch);
        assert_eq!(
            item,
            Item {
                field: 2,
                other: String::from("bye")
            }
        );
    }

    #[cfg(feature = "op")]
    #[test]
    fn test_add_patches() {
        #[derive(Patch)]
        #[patch(attribute(derive(Debug, PartialEq)))]
        struct Item {
            field: u32,
            other: String,
        }

        let patch = ItemPatch {
            field: Some(1),
            other: None,
        };
        let patch2 = ItemPatch {
            field: None,
            other: Some(String::from("hello")),
        };
        let overall_patch = patch + patch2;
        assert_eq!(
            overall_patch,
            ItemPatch {
                field: Some(1),
                other: Some(String::from("hello")),
            }
        );
    }

    #[cfg(feature = "op")]
    #[test]
    #[should_panic]
    fn test_add_conflict_patches_panic() {
        #[derive(Patch, Debug, PartialEq)]
        struct Item {
            field: u32,
        }

        let patch = ItemPatch { field: Some(1) };
        let patch2 = ItemPatch { field: Some(2) };
        let _overall_patch = patch + patch2;
    }

    #[cfg(feature = "merge")]
    #[test]
    fn test_merge() {
        #[allow(dead_code)]
        #[derive(Patch)]
        #[patch(attribute(derive(PartialEq, Debug)))]
        struct Item {
            a: u32,
            b: u32,
            c: u32,
            d: u32,
        }

        let patch = ItemPatch {
            a: None,
            b: Some(2),
            c: Some(0),
            d: None,
        };
        let patch2 = ItemPatch {
            a: Some(1),
            b: None,
            c: Some(3),
            d: None,
        };

        let merged_patch = patch.merge(patch2);
        assert_eq!(
            merged_patch,
            ItemPatch {
                a: Some(1),
                b: Some(2),
                c: Some(3),
                d: None,
            }
        );
    }

    #[cfg(feature = "merge")]
    #[test]
    fn test_merge_nested() {
        #[allow(dead_code)]
        #[derive(Patch, PartialEq, Debug)]
        #[patch(attribute(derive(PartialEq, Debug, Clone)))]
        struct B {
            c: u32,
            d: u32,
            e: u32,
            f: u32,
        }

        #[allow(dead_code)]
        #[derive(Patch)]
        #[patch(attribute(derive(PartialEq, Debug)))]
        struct A {
            a: u32,
            #[patch(name = "BPatch")]
            b: B,
        }

        let patches = alloc::vec![
            APatch {
                a: Some(1),
                b: Some(BPatch {
                    c: None,
                    d: Some(2),
                    e: Some(0),
                    f: None,
                }),
            },
            APatch {
                a: Some(0),
                b: Some(BPatch {
                    c: Some(1),
                    d: None,
                    e: Some(3),
                    f: None,
                }),
            },
        ];

        let merged_patch = patches.into_iter().reduce(Merge::merge).unwrap();

        assert_eq!(
            merged_patch,
            APatch {
                a: Some(0),
                b: Some(BPatch {
                    c: Some(1),
                    d: Some(2),
                    e: Some(3),
                    f: None,
                }),
            }
        );
    }

    /// `#[patch(no_diff)]` lets a struct derive `Patch` even when its fields
    /// cannot derive `PartialEq` (e.g. they hold `RefCell`, locks, or opaque
    /// runtime caches). The trait's default `into_patch_by_diff` runs instead,
    /// which just falls back to a full `into_patch()`.
    #[test]
    fn test_no_diff_skips_partial_eq_requirement() {
        use core::cell::RefCell;

        #[derive(Patch, Default)]
        #[patch(no_diff)]
        #[patch(attribute(derive(Debug, Default)))]
        struct HoldsRefCell {
            // RefCell<T> does not implement PartialEq, so deriving it on the
            // struct would fail. With `no_diff`, the macro never emits the
            // field-by-field diff that would require it.
            cache: RefCell<u32>,
            data: u32,
        }

        let mut item = HoldsRefCell::default();
        item.data = 7;
        let patch = HoldsRefCellPatch {
            cache: None,
            data: Some(42),
        };
        item.apply(patch);
        assert_eq!(item.data, 42);

        // into_patch_by_diff still works (via the trait default) — it just
        // falls back to a full patch instead of a diff.
        let prev = HoldsRefCell::default();
        let full_patch = item.into_patch_by_diff(prev);
        assert_eq!(full_patch.data, Some(42));
    }

    /// `#[patch(skip_serializing_none)]` keeps unset (`None`) patch fields
    /// out of the serialized JSON, so a sparse patch stays sparse on the wire.
    #[cfg(feature = "serde")]
    #[test]
    fn test_skip_serializing_none() {
        use serde::{Deserialize, Serialize};

        #[derive(Patch)]
        #[patch(skip_serializing_none)]
        #[patch(attribute(derive(Debug, PartialEq, Serialize, Deserialize)))]
        struct Item {
            a: u32,
            b: Option<String>,
        }

        let empty: ItemPatch = serde_json::from_str("{}").unwrap();
        assert_eq!(serde_json::to_string(&empty).unwrap(), "{}");

        let sparse: ItemPatch = serde_json::from_str(r#"{ "a": 1 }"#).unwrap();
        assert_eq!(serde_json::to_string(&sparse).unwrap(), r#"{"a":1}"#);

        // Plain `Option<T>` fields (no `nullable`) still cannot express
        // "clear": explicit null deserializes to the same `None` as a
        // missing key.
        let explicit_null: ItemPatch = serde_json::from_str(r#"{ "b": null }"#).unwrap();
        assert_eq!(explicit_null, ItemPatch { a: None, b: None });
    }

    /// `#[patch(nullable)]` makes `Option<T>` fields tri-state over JSON:
    /// missing = no change, explicit `null` = clear, value = set.
    #[cfg(feature = "serde")]
    #[test]
    fn test_nullable_serde_roundtrip() {
        use serde::{Deserialize, Serialize};

        #[derive(Patch, Debug, PartialEq)]
        #[patch(skip_serializing_none)]
        #[patch(attribute(derive(Debug, PartialEq, Serialize, Deserialize)))]
        struct Item {
            #[patch(nullable)]
            maybe: Option<String>,
            plain: u32,
        }

        // missing key -> None -> apply is a no-op
        let patch: ItemPatch = serde_json::from_str("{}").unwrap();
        let mut item = Item {
            maybe: Some("keep".into()),
            plain: 1,
        };
        item.apply(patch);
        assert_eq!(item.maybe, Some("keep".into()));

        // explicit null -> Some(None) -> apply clears the field
        let patch: ItemPatch = serde_json::from_str(r#"{ "maybe": null }"#).unwrap();
        let mut item = Item {
            maybe: Some("clear me".into()),
            plain: 1,
        };
        item.apply(patch);
        assert_eq!(item.maybe, None);

        // a value -> Some(Some(v)) -> apply sets the field
        let patch: ItemPatch = serde_json::from_str(r#"{ "maybe": "new" }"#).unwrap();
        let mut item = Item {
            maybe: None,
            plain: 1,
        };
        item.apply(patch);
        assert_eq!(item.maybe, Some("new".into()));

        // `None` (no change) must not serialize as `null` — otherwise a
        // round-trip would turn "no change" into "clear".
        let no_change = ItemPatch {
            maybe: None,
            plain: Some(1),
        };
        assert_eq!(serde_json::to_string(&no_change).unwrap(), r#"{"plain":1}"#);
        // ...while a real clear does serialize as explicit `null`.
        let clear = ItemPatch {
            maybe: Some(None),
            plain: None,
        };
        assert_eq!(serde_json::to_string(&clear).unwrap(), r#"{"maybe":null}"#);
    }
}
