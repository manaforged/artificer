use std::env;
use std::process::ExitCode;

const NEST: &str = "ARTIFICER_NEST";
const NEST_MAX: u32 = 16;

pub(super) fn enter() -> Option<ExitCode> {
    let nest = env::var(NEST)
        .ok()
        .and_then(|value| value.trim().parse::<u32>().ok())
        .unwrap_or(0);
    if nest >= NEST_MAX {
        eprintln!(
            "artificer: the Cargo shim started {nest} nested copies of itself; refusing to start another. Check that ARTIFICER_REAL_CARGO or ~/.artificer/real-cargo names the real Cargo, not this shim."
        );
        return Some(ExitCode::from(101));
    }
    // SAFETY: main calls this before it starts any other thread.
    unsafe { env::set_var(NEST, (nest + 1).to_string()) };
    None
}
