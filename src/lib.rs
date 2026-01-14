//! Transaction decoding for aggkit-proxy
//!
//! Decodes Ethereum transactions and parses claimAsset bridge calls.

pub mod decode;

pub use decode::{
    decode_transaction, parse_claim_asset, ClaimAssetParams, DecodeError, GlobalIndex,
};
