//! Forwarding shim for the clean-room `.TEAM` codec.
//!
//! The implementation and the host conversion live in `team_adapter`; this
//! module only preserves the existing `team_file::…` call sites.

pub(crate) use super::team_adapter::*;
