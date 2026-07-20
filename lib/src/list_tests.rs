//! Tests for the `list` feature (`#[patch(list_patch(...))]`).

#![cfg(all(test, feature = "list", feature = "serde"))]

extern crate alloc;
use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use serde::{Deserialize, Serialize};

use crate as struct_patch;
use crate::list::{ListPatchApply, ListPatchOp};
use crate::Patch;
#[cfg(feature = "status")]
use crate::Status;

#[derive(Clone, Debug, Default, PartialEq, Patch, Serialize, Deserialize)]
#[patch(attribute(derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq)))]
struct Inner {
    id: u32,
    value: u32,
    label: String,
}

#[derive(Clone, Debug, Default, PartialEq, Patch, Serialize, Deserialize)]
#[patch(attribute(derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq)))]
struct Container {
    name: String,
    #[patch(list_patch(id = |x| x.id, id_type = u32))]
    items: Vec<Inner>,
}

/// Direct unit tests for the `ListPatchOp` helpers in `lib/src/list.rs`.
#[test]
fn list_patch_op_apply_to_smoke() {
    let mut v: Vec<Inner> = vec![];
    let id_fn = |x: &Inner| x.id;

    ListPatchOp::<Inner, InnerPatch, u32>::append(Inner {
        id: 1,
        value: 10,
        label: "a".into(),
    })
    .apply_to(&mut v, id_fn);
    ListPatchOp::<Inner, InnerPatch, u32>::append(Inner {
        id: 2,
        value: 20,
        label: "b".into(),
    })
    .apply_to(&mut v, id_fn);
    assert_eq!(v.len(), 2);

    // Modify by id
    ListPatchOp::modify(
        1u32,
        InnerPatch {
            value: Some(100),
            ..Default::default()
        },
    )
    .apply_to(&mut v, id_fn);
    assert_eq!(v[0].value, 100);
    assert_eq!(v[0].label, "a");

    // Prepend
    ListPatchOp::<Inner, InnerPatch, u32>::prepend(Inner {
        id: 0,
        value: 0,
        label: "z".into(),
    })
    .apply_to(&mut v, id_fn);
    assert_eq!(v[0].id, 0);

    // Insert at index 2
    ListPatchOp::<Inner, InnerPatch, u32>::insert(
        2,
        Inner {
            id: 5,
            value: 50,
            label: "m".into(),
        },
    )
    .apply_to(&mut v, id_fn);
    assert_eq!(v[2].id, 5);

    // Insert out-of-range clamps to append
    ListPatchOp::<Inner, InnerPatch, u32>::insert(
        999,
        Inner {
            id: 9,
            value: 90,
            label: "end".into(),
        },
    )
    .apply_to(&mut v, id_fn);
    assert_eq!(v.last().unwrap().id, 9);

    // Delete by id
    ListPatchOp::<Inner, InnerPatch, u32>::delete(2u32).apply_to(&mut v, id_fn);
    assert!(v.iter().all(|x| x.id != 2));

    // Modify unknown id is silently ignored
    ListPatchOp::modify(
        999u32,
        InnerPatch {
            value: Some(7),
            ..Default::default()
        },
    )
    .apply_to(&mut v, id_fn);
}

/// A patch containing a list_patch field can be applied, mutating the list
/// in place.
#[test]
fn list_patch_apply_via_derive() {
    let mut container = Container {
        name: "orig".into(),
        items: vec![
            Inner {
                id: 1,
                value: 1,
                label: "one".into(),
            },
            Inner {
                id: 2,
                value: 2,
                label: "two".into(),
            },
        ],
    };

    let patch = ContainerPatch {
        name: Some("updated".into()),
        items: vec![
            // modify id=1
            ListPatchOp::modify(
                1u32,
                InnerPatch {
                    value: Some(11),
                    ..Default::default()
                },
            ),
            // delete id=2
            ListPatchOp::delete(2u32),
            // append a new id=3
            ListPatchOp::append(Inner {
                id: 3,
                value: 3,
                label: "three".into(),
            }),
            // prepend a new id=0
            ListPatchOp::prepend(Inner {
                id: 0,
                value: 0,
                label: "zero".into(),
            }),
        ],
    };

    container.apply(patch);

    assert_eq!(container.name, "updated");
    let ids: Vec<u32> = container.items.iter().map(|x| x.id).collect();
    // prepend(0), kept(1 with modified value), append(3)  (2 deleted)
    assert_eq!(ids, vec![0, 1, 3]);
    let one = container.items.iter().find(|x| x.id == 1).unwrap();
    assert_eq!(one.value, 11);
    assert_eq!(one.label, "one");
}

/// The serde representation matches the documented shape.
#[test]
#[cfg(feature = "serde")]
fn list_patch_serde_shape() {
    let op: ListPatchOp<Inner, InnerPatch, u32> = ListPatchOp::Append {
        value: Inner {
            id: 7,
            value: 70,
            label: "seven".into(),
        },
    };
    let s = serde_json::to_string(&op).unwrap();
    assert!(s.starts_with(r#"{"op":"append","#));
    let back: ListPatchOp<Inner, InnerPatch, u32> = serde_json::from_str(&s).unwrap();
    assert_eq!(op, back);

    let op = ListPatchOp::<Inner, InnerPatch, u32>::modify(
        42u32,
        InnerPatch {
            value: Some(4242),
            ..Default::default()
        },
    );
    let s = serde_json::to_string(&op).unwrap();
    assert!(s.starts_with(r#"{"op":"modify","#));
    assert!(s.contains(r#""id":42"#));

    let op = ListPatchOp::<Inner, InnerPatch, u32>::delete(5u32);
    let s = serde_json::to_string(&op).unwrap();
    assert_eq!(s, r#"{"op":"delete","id":5}"#);

    let op = ListPatchOp::<Inner, InnerPatch, u32>::insert(
        2,
        Inner {
            id: 1,
            value: 1,
            label: "i".into(),
        },
    );
    let s = serde_json::to_string(&op).unwrap();
    assert!(s.starts_with(r#"{"op":"insert","#));
    assert!(s.contains(r#""index":2"#));
}

/// Patching a whole struct from / to JSON.
#[test]
#[cfg(feature = "serde")]
fn list_patch_serde_roundtrip() {
    let data = r#"{
        "name": "patched",
        "items": [
            {"op":"prepend","value":{"id":10,"value":100,"label":"ten"}},
            {"op":"modify","id":1,"value":{"value":999}},
            {"op":"delete","id":2},
            {"op":"insert","index":1,"value":{"id":50,"value":500,"label":"fifty"}}
        ]
    }"#;

    let patch: ContainerPatch = serde_json::from_str(data).unwrap();
    assert_eq!(patch.items.len(), 4);

    let mut container = Container {
        name: "orig".into(),
        items: vec![
            Inner {
                id: 1,
                value: 1,
                label: "one".into(),
            },
            Inner {
                id: 2,
                value: 2,
                label: "two".into(),
            },
        ],
    };
    container.apply(patch);
    let ids: Vec<u32> = container.items.iter().map(|x| x.id).collect();
    // prepend(10), insert at 1(50), kept(1, modified), deleted(2)
    assert_eq!(ids, vec![10, 50, 1]);
    let one = container.items.iter().find(|x| x.id == 1).unwrap();
    assert_eq!(one.value, 999);
}

/// Empty patch (`new_empty_patch`) yields an empty Vec for list fields.
#[test]
fn list_patch_new_empty() {
    let p: ContainerPatch = Container::new_empty_patch();
    assert!(p.items.is_empty());

    #[cfg(feature = "status")]
    assert!(p.is_empty());
}

/// `into_patch` serialises an existing instance as a series of `Append` ops.
#[test]
fn list_patch_into_patch() {
    let container = Container {
        name: "x".into(),
        items: vec![
            Inner {
                id: 1,
                value: 1,
                label: "a".into(),
            },
            Inner {
                id: 2,
                value: 2,
                label: "b".into(),
            },
        ],
    };
    let patch: ContainerPatch = container.into_patch();
    assert_eq!(patch.items.len(), 2);
    for op in &patch.items {
        match op {
            ListPatchOp::Append { .. } => {}
            _ => panic!("expected only Append ops, got {:?}", op),
        }
    }
}

/// `into_patch_by_diff` produces a structural diff using ids.
#[test]
fn list_patch_into_patch_by_diff() {
    let previous = Container {
        name: "old".into(),
        items: vec![
            Inner {
                id: 1,
                value: 1,
                label: "a".into(),
            },
            Inner {
                id: 2,
                value: 2,
                label: "b".into(),
            },
            Inner {
                id: 3,
                value: 3,
                label: "c".into(),
            },
        ],
    };
    let current = Container {
        name: "new".into(),
        items: vec![
            // id=1 unchanged
            Inner {
                id: 1,
                value: 1,
                label: "a".into(),
            },
            // id=2 modified
            Inner {
                id: 2,
                value: 22,
                label: "b".into(),
            },
            // id=4 added
            Inner {
                id: 4,
                value: 4,
                label: "d".into(),
            },
            // id=3 deleted
        ],
    };

    let patch: ContainerPatch = current.clone().into_patch_by_diff(previous);

    // Name changed too
    assert_eq!(patch.name, Some("new".into()));

    let mut n_append = 0;
    let mut n_delete = 0;
    let mut n_modify = 0;
    for op in &patch.items {
        match op {
            ListPatchOp::Append { value } => {
                assert_eq!(value.id, 4);
                n_append += 1;
            }
            ListPatchOp::Delete { id } => {
                assert_eq!(*id, 3);
                n_delete += 1;
            }
            ListPatchOp::Modify { id, value } => {
                // Every matched element gets a `Modify`; unchanged ones carry
                // an (idempotent) empty sub-patch.
                if *id == 2 {
                    assert_eq!(value.value, Some(22));
                } else {
                    assert_eq!(*id, 1);
                    assert_eq!(value.value, None);
                }
                n_modify += 1;
            }
            _ => {}
        }
    }
    assert_eq!(n_append, 1);
    assert_eq!(n_delete, 1);
    assert_eq!(n_modify, 2);

    // Apply the diff to the original; should reconstruct `current`
    let mut state = Container {
        name: "old".into(),
        items: vec![
            Inner {
                id: 1,
                value: 1,
                label: "a".into(),
            },
            Inner {
                id: 2,
                value: 2,
                label: "b".into(),
            },
            Inner {
                id: 3,
                value: 3,
                label: "c".into(),
            },
        ],
    };
    state.apply(patch);
    // Compare item-by-item by id rather than order-sensitive, since delete +
    // append don't preserve position.
    let mut actual: Vec<(u32, u32, String)> = state
        .items
        .iter()
        .map(|i| (i.id, i.value, i.label.clone()))
        .collect();
    actual.sort_by_key(|x| x.0);
    let mut expected: Vec<(u32, u32, String)> = current
        .items
        .iter()
        .map(|i| (i.id, i.value, i.label.clone()))
        .collect();
    expected.sort_by_key(|x| x.0);
    assert_eq!(actual, expected);
    assert_eq!(state.name, "new");
}

/// Recursive (nested) list patching: a list element is itself a patchable
/// struct whose patch can again contain nested fields. The `Modify` op
/// carries an `InnerPatch`, which already supports recursion via the
/// ordinary Patch derive.
#[test]
#[cfg(feature = "nesting")]
fn list_patch_recursive_modify() {
    #[derive(Clone, Debug, Default, PartialEq, Patch, Serialize, Deserialize)]
    #[patch(attribute(derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq)))]
    struct Deep {
        n: u32,
    }

    #[derive(Clone, Debug, Default, PartialEq, Patch, Serialize, Deserialize)]
    #[patch(attribute(derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq)))]
    struct Middle {
        id: u32,
        #[patch(nesting)]
        deep: Deep,
    }

    #[derive(Clone, Debug, Default, PartialEq, Patch, Serialize, Deserialize)]
    #[patch(attribute(derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq)))]
    struct Outer {
        #[patch(list_patch(id = |x| x.id, id_type = u32))]
        children: Vec<Middle>,
    }

    let mut outer = Outer {
        children: vec![
            Middle {
                id: 1,
                deep: Deep { n: 10 },
            },
            Middle {
                id: 2,
                deep: Deep { n: 20 },
            },
        ],
    };

    let patch = OuterPatch {
        children: vec![ListPatchOp::modify(
            1u32,
            MiddlePatch {
                id: None,
                deep: DeepPatch { n: Some(100) },
            },
        )],
    };

    outer.apply(patch);
    assert_eq!(outer.children[0].deep.n, 100);
    assert_eq!(outer.children[1].deep.n, 20);
}

/// Unknown id in `Modify` and `Delete` is silently ignored; we don't panic.
#[test]
fn list_patch_unknown_id_ignored() {
    let mut container = Container {
        name: "x".into(),
        items: vec![Inner {
            id: 1,
            value: 1,
            label: "one".into(),
        }],
    };

    let patch = ContainerPatch {
        name: None,
        items: vec![
            ListPatchOp::delete(999u32),
            ListPatchOp::modify(
                999u32,
                InnerPatch {
                    value: Some(7),
                    ..Default::default()
                },
            ),
        ],
    };
    container.apply(patch);
    assert_eq!(container.items.len(), 1);
    assert_eq!(container.items[0].id, 1);
}

/// Sanity-check the public `ListPatchApply` trait used by the generated code.
#[test]
fn list_patch_apply_trait_object_safe() {
    let mut v: Vec<Inner> = vec![Inner {
        id: 1,
        value: 1,
        label: "a".into(),
    }];
    let ops: Vec<ListPatchOp<Inner, InnerPatch, u32>> = vec![
        ListPatchOp::Append {
            value: Inner {
                id: 2,
                value: 2,
                label: "b".into(),
            },
        },
        ListPatchOp::Delete { id: 1 },
    ];

    <Vec<Inner> as ListPatchApply>::apply_list_patch_ops(&mut v, ops, |x| x.id);
    assert_eq!(v.len(), 1);
    assert_eq!(v[0].id, 2);
}

/// Smoke test: a custom `patch_type` override works.
#[test]
fn list_patch_custom_patch_type() {
    // Re-derive Inner with a renamed patch via the `name` attribute.
    #[derive(Clone, Debug, Default, PartialEq, Patch, Serialize, Deserialize)]
    #[patch(name = "InnerItemPatch")]
    #[patch(attribute(derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq)))]
    struct InnerItem {
        id: u32,
        value: u32,
    }

    #[derive(Clone, Debug, Default, PartialEq, Patch)]
    struct Holder {
        #[patch(list_patch(
            id = |x| x.id,
            id_type = u32,
            patch_type = InnerItemPatch
        ))]
        items: Vec<InnerItem>,
    }

    let mut h = Holder {
        items: vec![InnerItem { id: 1, value: 1 }],
    };
    let patch = HolderPatch {
        items: vec![
            ListPatchOp::modify(
                1u32,
                InnerItemPatch {
                    value: Some(42),
                    ..Default::default()
                },
            ),
            ListPatchOp::append(InnerItem { id: 2, value: 2 }),
        ],
    };
    h.apply(patch);
    assert_eq!(h.items.len(), 2);
    assert_eq!(h.items[0].value, 42);
    assert_eq!(h.items[1].id, 2);
}

#[test]
#[cfg(feature = "status")]
fn list_patch_status_impl() {
    let empty = ContainerPatch {
        name: None,
        items: vec![],
    };
    assert!(empty.is_empty());

    let non_empty = ContainerPatch {
        name: None,
        items: vec![ListPatchOp::delete(1u32)],
    };
    assert!(!non_empty.is_empty());
}

#[test]
#[cfg(feature = "op")]
fn list_patch_add_concatenates() {
    let p1 = ContainerPatch {
        name: None,
        items: vec![ListPatchOp::delete(1u32)],
    };
    let p2 = ContainerPatch {
        name: None,
        items: vec![ListPatchOp::delete(2u32)],
    };
    let combined = p1 + p2;
    assert_eq!(combined.items.len(), 2);
}

#[test]
#[cfg(feature = "merge")]
fn list_patch_merge_concatenates() {
    use crate::Merge;
    let p1 = ContainerPatch {
        name: None,
        items: vec![ListPatchOp::delete(1u32)],
    };
    let p2 = ContainerPatch {
        name: None,
        items: vec![ListPatchOp::delete(2u32)],
    };
    let combined = p1.merge(p2);
    assert_eq!(combined.items.len(), 2);
}

/// Compile-time check: `format!` works (sanity that `alloc::string::String`
/// is wired up).
#[test]
fn format_works() {
    assert_eq!(format!("{}", 1), "1");
}
