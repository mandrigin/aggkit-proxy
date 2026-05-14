//! aggkit-proxy library — supplanted by gateway-fm/miden-agglayer.
//!
//! This crate originally hosted a vibecoded JSON-RPC proxy that bridged
//! Ethereum-style RPC to the Miden network. That implementation has been
//! replaced in production by `gateway-fm/miden-agglayer`, which is what the
//! Kurtosis package (`kurtosis/miden-cdk/`) actually deploys as the
//! `miden-proxy` service.
//!
//! What lives here now:
//! - `tests/phase{1,2,3}.rs` — miden-client integration tests (no proxy
//!   involvement; they exercise upstream client primitives directly).
//! - `src/bin/{verify_notes,claim_note}.rs` — operator helpers built against
//!   miden-client.
//! - `kurtosis/miden-cdk/` — Starlark package that deploys miden-agglayer
//!   alongside aggkit, bridge-service, agglayer, and a Miden node.
//!
//! Anything else this lib used to export (address mapping, claim tracker,
//! decode, receipt synthesis, log synthesis, faucet creation, etc.) now
//! lives in `gateway-fm/miden-agglayer`.
