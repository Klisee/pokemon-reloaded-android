//! Compatibility adapters for content-specific native identifiers.
//!
//! The core must not contain F3R room/object/script/global/save identifiers. They
//! live in [`f3r`], the versioned manifest for the supported build family.

pub mod f3r;
