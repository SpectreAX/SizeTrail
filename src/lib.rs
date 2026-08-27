#![forbid(clippy::disallowed_methods)]
#![forbid(
    clippy::let_underscore_must_use,
    clippy::let_underscore_untyped,
    clippy::unwrap_used
)]
#![deny(clippy::disallowed_types)]

pub mod capacity;
pub mod fsx;
pub mod model;
pub mod policy;
pub mod scan;
