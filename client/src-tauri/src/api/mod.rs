//! The webview's way to the binone server's GraphQL: one command, `api_call`, that takes an
//! operation name and arguments.
//!
//! - `ops`: the allowlist, the argument checks and the fixed documents (pure).
//! - `response`: reading an answer and sorting a failure (pure).
//! - `error`: the error type and the removal of the key (pure).
//! - `gateway`: the HTTP call with the saved address and key, and the one key replacement on 401.

mod error;
mod gateway;
mod ops;
mod response;

pub use error::ApiError;
pub use gateway::Gateway;
