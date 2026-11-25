//! Omnia's unattended benchmark harness (plan sections 2, 8 and 9; roadmap T0.3).
//!
//! The `bench` binary (`cargo run --release -p omnia-bench --`) installs
//! [`alloc::CountingAlloc`] as the global allocator and dispatches to the subcommands here.
//! Every report is JSON with `schema`, `kind`, `config`, `environment` and `metrics`; every
//! number under `metrics` is a cost (lower is better) and is what `bench compare` checks.

pub mod alloc;
pub mod compare;
pub mod drag_part;
pub mod driver;
pub mod generate;
pub mod harness;
pub mod hover;
pub mod paint_probe;
pub mod scene;
#[cfg(feature = "headless")]
pub mod scripts;
pub mod spike;
pub mod start;
pub mod stats;
pub mod sys;
mod trace;

pub use alloc::{CountingAlloc, count_allocs};

#[cfg(test)]
#[global_allocator]
static TEST_ALLOC: CountingAlloc = CountingAlloc;

pub mod hit;
