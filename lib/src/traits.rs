/// A struct that a patch can be applied to
///
/// Deriving [`Patch`] will generate a patch struct and an accompanying trait impl so that it can be applied to the original struct.
/// ```rust
/// # use struct_patch::Patch;
/// #[derive(Patch)]
/// struct Item {
///     field_bool: bool,
///     field_int: usize,
///     field_string: String,
/// }
///
/// // Generated struct
/// // struct ItemPatch {
/// //     field_bool: Option<bool>,
/// //     field_int: Option<usize>,
/// //     field_string: Option<String>,
/// // }
/// ```
/// ## Container attributes
/// ### `#[patch(attribute(derive(...)))]`
/// Use this attribute to derive traits on the generated patch struct
/// ```rust
/// # use struct_patch::Patch;
/// # use serde::{Serialize, Deserialize};
/// #[derive(Patch)]
/// #[patch(attribute(derive(Debug, Default, Deserialize, Serialize)))]
/// struct Item;
///
/// // Generated struct
/// // #[derive(Debug, Default, Deserialize, Serialize)]
/// // struct ItemPatch {}
/// ```
///
/// ### `#[patch(attribute(...))]`
/// Use this attribute to pass the attributes on the generated patch struct
/// ```compile_fail
/// // This example need `serde` and `serde_with` crates
/// # use struct_patch::Patch;
/// #[derive(Patch, Debug)]
/// #[patch(attribute(derive(Serialize, Deserialize, Default)))]
/// #[patch(attribute(skip_serializing_none))]
/// struct Item;
///
/// // Generated struct
/// // #[derive(Default, Deserialize, Serialize)]
/// // #[skip_serializing_none]
/// // struct ItemPatch {}
/// ```
///
/// ### `#[patch(skip_serializing_none)]`
/// Add `#[serde(skip_serializing_if = "Option::is_none")]` to every
/// `Option`-wrapped patch field, so unset fields disappear from the
/// serialized patch instead of showing up as explicit `null`s. This is the
/// built-in equivalent of `serde_with`'s `skip_serializing_none` (shown
/// above) and needs no extra dependency. Fields with their own
/// serialization semantics (`empty_value`, `skip_wrap`, `nesting`,
/// `list_patch`) are unaffected, as are `nullable` fields (which already
/// carry the attribute). The patch struct must derive `serde::Serialize`.
/// ```rust
/// # use struct_patch::Patch;
/// # use serde::Serialize;
/// #[derive(Patch)]
/// #[patch(skip_serializing_none)]
/// #[patch(attribute(derive(Serialize)))]
/// struct Item {
///     field_int: usize,
/// }
///
/// // Generated struct
/// // #[derive(Serialize)]
/// // struct ItemPatch {
/// //     #[serde(skip_serializing_if = "Option::is_none")]
/// //     field_int: Option<usize>,
/// // }
///
/// let patch = ItemPatch { field_int: None };
/// assert_eq!(serde_json::to_string(&patch).unwrap(), "{}");
/// ```
///
/// ### `#[patch(name = "...")]`
/// Use this attribute to change the name of the generated patch struct
/// ```rust
/// # use struct_patch::Patch;
/// #[derive(Patch)]
/// #[patch(name = "ItemOverlay")]
/// struct Item { }
///
/// // Generated struct
/// // struct ItemOverlay {}
/// ```
///
/// ## Field attributes
/// ### `#[patch(skip)]`
/// If you want certain fields to be unpatchable, you can let the derive macro skip certain fields when creating the patch struct
/// ```rust
/// # use struct_patch::Patch;
/// #[derive(Patch)]
/// struct Item {
///     #[patch(skip)]
///     id: String,
///     data: String,
/// }
///
/// // Generated struct
/// // struct ItemPatch {
/// //     data: Option<String>,
/// // }
/// ```
///
/// ### `#[patch(skip_wrap)]`
/// Keep the field type as-is in the generated patch struct (no extra `Option`
/// wrapping). This is useful for fields that are already `Option<...>`,
/// typically `Option<Vec<_>>`, where the default double-`Option` in the patch
/// is unwanted. With `skip_wrap`, `None` in the patch means "no change" and
/// `Some(v)` sets the field to `Some(v)` (including `Some(vec![])` to clear
/// the vector). Cannot be combined with `empty_value`.
/// ```rust
/// # use struct_patch::Patch;
/// #[derive(Default, Patch)]
/// struct Item {
///     #[patch(skip_wrap)]
///     tags: Option<Vec<String>>,
/// }
///
/// // Generated struct
/// // struct ItemPatch {
/// //     tags: Option<Vec<String>>, // not wrapped again
/// // }
///
/// let mut item = Item { tags: Some(vec!["a".into()]) };
///
/// // `None` in the patch keeps the field unchanged.
/// item.apply(ItemPatch { tags: None });
/// assert_eq!(item.tags, Some(vec!["a".into()]));
///
/// // `Some(vec![])` still applies and clears the list.
/// item.apply(ItemPatch { tags: Some(vec![]) });
/// assert_eq!(item.tags, Some(vec![]));
/// ```
///
/// ### `#[patch(nullable)]`
/// Make an `Option<T>` field tri-state over serde. The patch field keeps the
/// double-`Option` type (`Option<Option<T>>`), and the derive emits the serde
/// plumbing that plain `Option<Option<T>>` lacks:
///
/// | wire            | patch value     | effect on apply |
/// | --------------- | --------------- | --------------- |
/// | key missing     | `None`          | no change       |
/// | explicit `null` | `Some(None)`    | clear the field |
/// | a value         | `Some(Some(v))` | set the field   |
///
/// Without `nullable`, an explicit `null` deserializes to `None`, so "clear"
/// is indistinguishable from "no change". Requires the patch struct to
/// derive `serde::Serialize`/`serde::Deserialize`. Cannot be combined with
/// `skip_wrap`, `empty_value`, `nesting` or `list_patch`.
/// ```rust
/// # use struct_patch::Patch;
/// # use serde::{Serialize, Deserialize};
/// #[derive(Default, Patch)]
/// #[patch(attribute(derive(Serialize, Deserialize)))]
/// struct Item {
///     #[patch(nullable)]
///     nickname: Option<String>,
/// }
///
/// // Generated struct
/// // struct ItemPatch {
/// //     #[serde(default, skip_serializing_if = "Option::is_none",
/// //             deserialize_with = "struct_patch::serde_utils::deserialize_some")]
/// //     nickname: Option<Option<String>>,
/// // }
///
/// let mut item = Item { nickname: Some("a".into()) };
///
/// // Explicit `null` clears the field.
/// let patch: ItemPatch = serde_json::from_str(r#"{ "nickname": null }"#).unwrap();
/// item.apply(patch);
/// assert_eq!(item.nickname, None);
///
/// // A missing key is a no-op, and stays out of the serialized patch.
/// let patch: ItemPatch = serde_json::from_str(r#"{}"#).unwrap();
/// assert_eq!(serde_json::to_string(&patch).unwrap(), "{}");
/// item.apply(patch);
/// assert_eq!(item.nickname, None);
/// ```
pub trait Patch<P> {
    /// Apply a patch
    fn apply(&mut self, patch: P);

    /// Returns a patch that when applied turns any struct of the same type into `Self`
    fn into_patch(self) -> P;

    /// Returns a patch that when applied turns `previous_struct` into `Self`.
    ///
    /// requires `Self: PartialEq`. Types whose fields cannot derive
    /// `PartialEq` (e.g. they contain `RefCell`, locks, or opaque runtime
    /// caches) can opt out via `#[patch(no_diff)]`; the derive then skips the
    /// override and this default runs instead, falling back to a full patch.
    fn into_patch_by_diff(self, previous_struct: Self) -> P
    where
        Self: Sized,
    {
        let _ = previous_struct;
        self.into_patch()
    }

    /// Get an empty patch instance
    fn new_empty_patch() -> P;
}

pub trait Filler<F> {
    /// Apply a filler
    fn apply(&mut self, filler: F);

    /// Get an empty filler instance
    fn new_empty_filler() -> F;
}

#[cfg(feature = "status")]
/// A patch struct with extra status information
pub trait Status {
    /// Returns `true` if all fields are `None`, `false` otherwise.
    fn is_empty(&self) -> bool;
}

#[cfg(feature = "merge")]
/// A patch struct that can be merged to another one
pub trait Merge {
    fn merge(self, other: Self) -> Self;
}

#[cfg(feature = "catalyst")]
/// A substrate struct that can expose the fields information thereof
pub trait Substrate {
    fn expose_content() -> &'static str;

    /// Expose the field information, by call this function in Build.rs
    fn expose();
}

#[cfg(feature = "catalyst")]
/// A catalyst struct that can expose the fields information thereof
pub trait Catalyst<S, Cpx> {
    /// catalyst bind on substrate and generate complex
    fn bind(self, substrate: S) -> Cpx;
}

#[cfg(feature = "catalyst")]
/// A complex struct that can decouple return catalyst and substrate
pub trait Complex<Cat, S> {
    /// complex decouple to catalyst and substrate
    fn decouple(self) -> (Cat, S);
}
