#![deny(missing_docs)]
#![allow(non_snake_case)]
//! Musig implementation

#[macro_use]
extern crate failure;

mod context;
mod counterparty;
mod deferred_verification;
mod key;
mod signature;
mod signer;

mod errors;
mod transcript;

pub use self::context::{Multikey, Multimessage, MusigContext};
pub use self::deferred_verification::DeferredVerification;
pub use self::errors::MusigError;
pub use self::key::VerificationKey;
pub use self::signature::Signature;
pub use self::signer::{
    Signer, SignerAwaitingCommitments, SignerAwaitingPrecommitments, SignerAwaitingShares,
};
