mod auto_update;
mod check_tag_is_pushed;
mod find_repos;
mod remote_tags;
mod s;
mod sync;
mod tag_timestamp;
mod undelete;
mod web;

use std::process::ExitCode;

use anyhow::Result;

pub use crate::tools::git::auto_update::AutoUpdate;
pub use crate::tools::git::check_tag_is_pushed::CheckTagIsPushed;
pub use crate::tools::git::find_repos::FindRepos;
pub use crate::tools::git::remote_tags::RemoteTags;
pub use crate::tools::git::s::S;
pub use crate::tools::git::sync::Sync;
pub use crate::tools::git::tag_timestamp::TagTimestamp;
pub use crate::tools::git::undelete::Undelete;
pub use crate::tools::git::web::Web;

fn exit_code(result: Result<()>) -> ExitCode {
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("Error: {error:?}");
            ExitCode::FAILURE
        }
    }
}
