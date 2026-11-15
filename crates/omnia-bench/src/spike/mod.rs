//! `bench spike <scene>`: the day-1 spike scenes (plan section 8).

pub mod s7;

/// Why a spike command failed: bad arguments (exit 2) or a failed run (exit 3).
#[derive(Debug)]
pub enum SpikeError {
    Usage(String),
    Run(String),
}

/// Runs `bench spike <scene> [options]`; `args` starts at the scene name.
pub fn run(args: Vec<String>) -> Result<(), SpikeError> {
    let mut args = args.into_iter();
    match args.next().as_deref() {
        Some("s7") => s7::main(args.collect()),
        Some(other) => Err(SpikeError::Usage(format!("unknown spike scene `{other}`"))),
        None => Err(SpikeError::Usage("spike needs a scene name".into())),
    }
}
