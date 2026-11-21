//! Write and identify Microsoft-compatible boot records: a port of ms-sys
//! 2.9.0 (SVN trunk r156), the C program called "upstream" in this module.
//!
//! - The bytes written to the device are those upstream writes for the same
//!   request. The tests in `tests/api.rs` spell them out; an expected image
//!   there changes only if the port was wrong about upstream.
//! - The command line keeps upstream's switch names but is an ordinary clap
//!   parser, and the messages and exit status are the port's own: what was
//!   done or found goes to stdout, what went wrong to stderr with status 1.
//! - Upstream bugs that affect what is written are replicated, each marked
//!   with a comment starting `UPSTREAM-BUG:`.
//! - `payloads/*.bin` are the byte arrays of upstream's `inc/*.h`, copied
//!   unchanged and named after their header.
//! - `apply`, `inspect` and `smart_select` take a `Device` and return
//!   values; only `cli.rs`, `render.rs` and this file know about arguments,
//!   wording and exit status. The tests run them on a `MemoryDevice`, which
//!   carries a `Probe` saying what the device ioctls would report.

use std::io::{self, Write};
use std::os::unix::ffi::OsStrExt;
use std::process::ExitCode;

macro_rules! put {
    ($out:expr, $($piece:expr),+ $(,)?) => {{
        $( let _ = $out.write_all(AsRef::<[u8]>::as_ref(&$piece)); )+
    }};
}

mod apply;
mod cli;
mod device;
mod exfat;
mod identify;
mod payloads;
mod record;
mod render;
#[cfg(test)]
mod tests;

use apply::apply;
use cli::Selection;
use device::{Device, FileDevice};
use identify::{inspect, smart_select};

use crate::tools::Tool;

pub use cli::Invocation as MsSys;

impl Tool for MsSys {
    fn run(self) -> ExitCode {
        let mut out = io::BufWriter::new(io::stdout().lock());
        let mut err = io::stderr().lock();
        let status = run(&self, &mut out, &mut err);
        let _ = out.flush();
        ExitCode::from(status)
    }
}

/// Opens the device and does what was asked of it. What was asked for goes
/// to `out`, what went wrong to `err`; the return value is the exit status.
fn run(invocation: &MsSys, out: &mut dyn Write, err: &mut dyn Write) -> u8 {
    match FileDevice::open(invocation.device.as_os_str(), invocation.writes()) {
        Ok(mut device) => execute(invocation, &mut device, out, err),
        Err(error) => {
            let path = invocation.device.as_os_str().as_bytes();
            render::unopenable(err, path, &describe(&error));
            1
        }
    }
}

fn execute(
    invocation: &MsSys,
    device: &mut dyn Device,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> u8 {
    let path = invocation.device.as_os_str().as_bytes();
    let mut unselected = false;
    let record = match invocation.selection() {
        None => None,
        Some(Selection::Record(kind)) => Some(kind),
        Some(Selection::Auto) => {
            let selected = smart_select(device);
            if selected.is_none() {
                // UPSTREAM-BUG: after this message the program carries on
                // with whatever else it was asked to write, as if no record
                // type had been given. Upstream also exits with status 0.
                render::nothing_selected(err, path);
                unselected = true;
            }
            selected
        }
    };
    let request = invocation.request(record);
    if request.writes_nothing() {
        if unselected {
            return 1;
        }
        render::diagnosis(out, path, &inspect(device));
        return 0;
    }
    match apply(device, &request) {
        Ok(report) => {
            render::report(out, err, path, invocation, &report);
            u8::from(unselected || !report.succeeded())
        }
        Err(refusal) => {
            render::refusal(err, path, refusal);
            1
        }
    }
}

fn describe(error: &io::Error) -> Vec<u8> {
    match error.raw_os_error() {
        // SAFETY: `strerror` returns a valid NUL-terminated string. It may
        // be overwritten by a later call, so it is copied at once; no other
        // thread of this program calls it.
        Some(code) => unsafe { std::ffi::CStr::from_ptr(libc::strerror(code)) }
            .to_bytes()
            .to_vec(),
        None => error.to_string().into_bytes(),
    }
}
