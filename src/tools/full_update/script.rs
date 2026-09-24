use std::ffi::{OsStr, OsString};
use std::fmt;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct ScriptName(OsString);

impl ScriptName {
    pub const DOTFILES: &str = "00-dotfiles";

    pub fn as_os_str(&self) -> &OsStr {
        &self.0
    }
}

impl From<OsString> for ScriptName {
    fn from(name: OsString) -> Self {
        ScriptName(name)
    }
}

impl From<&str> for ScriptName {
    fn from(name: &str) -> Self {
        ScriptName(name.into())
    }
}

impl fmt::Display for ScriptName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.display().fmt(f)
    }
}
