//! Serde helpers used by code generated from `#[derive(Patch)]` field
//! attributes. Available with the `serde` feature.

use serde::{Deserialize, Deserializer};

/// Deserialize a *present* value as `Some(..)`.
///
/// Plain `Option<Option<T>>` cannot round-trip through serde: a missing key
/// and an explicit `null` both deserialize to `None`. Patch fields marked
/// with `#[patch(nullable)]` need to tell the three states apart:
///
/// | wire            | patch value    | meaning  |
/// | --------------- | -------------- | -------- |
/// | key missing     | `None`         | no change (via `#[serde(default)]`) |
/// | explicit `null` | `Some(None)`   | clear the field |
/// | a value         | `Some(Some(v))`| set the field |
///
/// Combining this deserializer with `#[serde(default,
/// skip_serializing_if = "Option::is_none")]` (all emitted by
/// `#[patch(nullable)]`) gives exactly that table.
pub fn deserialize_some<'de, T, D>(deserializer: D) -> Result<Option<T>, D::Error>
where
    T: Deserialize<'de>,
    D: Deserializer<'de>,
{
    T::deserialize(deserializer).map(Some)
}
