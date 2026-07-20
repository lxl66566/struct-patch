//! List patching support.
//!
//! When a struct field is a `Vec<T>` and `T` itself is patchable, the field
//! can be patched with fine-grained operations: prepend, append, insert-at,
//! modify-by-id and delete-by-id. The derive macro generates a patch field of
//! type `Vec<ListPatchOp<T, TPatch, ID>>` for such fields, and the apply logic
//! invokes [`ListPatchOp::apply_to`] with the user-supplied id closure.
//!
//! Serialised form (with the `serde` feature):
//! ```json
//! {"op":"prepend","value":{...}}
//! {"op":"append","value":{...}}
//! {"op":"insert","index":1,"value":{...}}
//! {"op":"modify","id":123,"value":{...}}
//! {"op":"delete","id":123}
//! ```

use crate::Patch;

extern crate alloc;
use alloc::vec::Vec;

/// A single fine-grained operation on a list field.
///
/// `T` is the element type, `TPatch` is its patch type, and `ID` is the
/// identifier type used to address individual elements.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(tag = "op", rename_all = "snake_case"))]
pub enum ListPatchOp<T, TPatch, ID> {
    /// Insert `value` at the front of the list.
    Prepend { value: T },
    /// Push `value` to the back of the list.
    Append { value: T },
    /// Insert `value` at the given index. Clamps to `vec.len()` so an
    /// out-of-range index is treated as an append.
    Insert { index: usize, value: T },
    /// Find the element whose id matches and apply a sub-patch to it.
    Modify { id: ID, value: TPatch },
    /// Remove the element whose id matches.
    Delete { id: ID },
}

impl<T, TPatch, ID> ListPatchOp<T, TPatch, ID> {
    /// Build a `Prepend` op.
    pub fn prepend(value: T) -> Self {
        ListPatchOp::Prepend { value }
    }

    /// Build an `Append` op.
    pub fn append(value: T) -> Self {
        ListPatchOp::Append { value }
    }

    /// Build an `Insert` op at the given index.
    pub fn insert(index: usize, value: T) -> Self {
        ListPatchOp::Insert { index, value }
    }

    /// Build a `Delete` op. Available without the `Patch` bound on `T`.
    pub fn delete(id: ID) -> Self {
        ListPatchOp::Delete { id }
    }
}

impl<T, TPatch, ID> ListPatchOp<T, TPatch, ID>
where
    T: Patch<TPatch>,
    ID: PartialEq,
{
    /// Build a `Modify` op.
    pub fn modify(id: ID, patch: TPatch) -> Self {
        ListPatchOp::Modify { id, value: patch }
    }

    /// Apply this single op to a `Vec<T>`, using `id_fn` to compute ids.
    ///
    /// Unknown ids in `Modify` / `Delete` are silently ignored.
    /// An out-of-range `index` in `Insert` clamps to `vec.len()`.
    pub fn apply_to(self, vec: &mut Vec<T>, id_fn: impl Fn(&T) -> ID) {
        match self {
            ListPatchOp::Prepend { value } => vec.insert(0, value),
            ListPatchOp::Append { value } => vec.push(value),
            ListPatchOp::Insert { index, value } => {
                let idx = index.min(vec.len());
                vec.insert(idx, value);
            }
            ListPatchOp::Modify { id, value: patch } => {
                if let Some(item) = vec.iter_mut().find(|x| id_fn(x) == id) {
                    item.apply(patch);
                }
            }
            ListPatchOp::Delete { id } => {
                vec.retain(|x| id_fn(x) != id);
            }
        }
    }
}

/// Helper trait used by generated code so that the `T: Patch<TPatch>` bound
/// only kicks in when actually applying list patches.
pub trait ListPatchApply {
    /// Element type (the `T` in `Vec<T>`).
    type Elem;

    /// Apply a sequence of list-patch ops to `self`.
    fn apply_list_patch_ops<TPatch, ID>(
        &mut self,
        ops: Vec<ListPatchOp<Self::Elem, TPatch, ID>>,
        id_fn: impl Fn(&Self::Elem) -> ID,
    ) where
        Self::Elem: Patch<TPatch>,
        ID: PartialEq;
}

impl<T> ListPatchApply for Vec<T> {
    type Elem = T;

    fn apply_list_patch_ops<TPatch, ID>(
        &mut self,
        ops: Vec<ListPatchOp<T, TPatch, ID>>,
        id_fn: impl Fn(&T) -> ID,
    ) where
        T: Patch<TPatch>,
        ID: PartialEq,
    {
        for op in ops {
            op.apply_to(self, &id_fn);
        }
    }
}
