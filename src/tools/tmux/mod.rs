mod attach;
mod attach_ssh;
mod chooser;
mod pane_title;
mod ssh;

use std::process::ExitCode;

use anyhow::Result;

pub use crate::tools::tmux::attach::Attach;
pub use crate::tools::tmux::attach_ssh::AttachSsh;
pub use crate::tools::tmux::chooser::Chooser;
pub use crate::tools::tmux::pane_title::PaneTitle;
pub use crate::tools::tmux::ssh::Ssh;

fn exit_code(result: Result<()>) -> ExitCode {
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("Error: {error:?}");
            ExitCode::FAILURE
        }
    }
}
