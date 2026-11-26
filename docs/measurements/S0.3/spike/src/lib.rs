//! S0.3 / SS1 spike crate: ngspice worker isolation, binary IPC streaming, and
//! cancel/kill benchmarking against in-process `libngspice`.

pub mod circuits;
pub mod ngspice;
pub mod protocol;
pub mod worker_client;
