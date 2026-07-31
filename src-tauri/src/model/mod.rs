//! Types mirroring Homebrew's JSON contracts.
//!
//! Two shapes exist for a reason. [`Entry`] is the lean record we keep in
//! memory for all ~16,000 packages so search stays instant; the full
//! [`detail::Formula`] / [`detail::Cask`] are fetched one at a time from
//! `brew info --json=v2`, which — unlike the published catalog — also knows
//! what is installed locally.

pub mod detail;
pub mod entry;
pub mod outdated;
pub mod service;

pub use detail::{Cask, Detail, Formula};
pub use entry::{Entry, Kind};
pub use outdated::{Outdated, OutdatedCask, OutdatedFormula};
pub use service::Service;
