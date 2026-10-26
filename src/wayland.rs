use std::env;
use std::ffi::OsString;

pub const WAYLAND_DISPLAY: &str = "WAYLAND_DISPLAY";

pub fn current_display() -> Option<OsString> {
    env::var_os(WAYLAND_DISPLAY).filter(|value| !value.is_empty())
}
