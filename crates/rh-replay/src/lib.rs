//! Deterministic run harness: share codes, command logs, and replay execution.
//!
//! A run is fully described by a base seed plus the semantic command log; the
//! pair round-trips through a compact share code usable in both clients.
