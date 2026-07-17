//! Shared UI-agnostic client layer.
//!
//! The session state machine, viewmodel, and input intents consumed by both
//! the terminal client and the WASM web client, so each stays a thin renderer
//! over identical simulation state and semantic commands.
