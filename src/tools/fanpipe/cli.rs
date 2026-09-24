use clap::Args as ClapArgs;

use crate::tools::fanpipe::config::{Config, Message, Mode, Until};
use crate::tools::fanpipe::topic::Topic;

#[derive(ClapArgs)]
#[command(about = "Fan messages out to subscribers over named pipes")]
pub struct Fanpipe {
    #[command(flatten)]
    mode: ModeArgs,
    #[arg(short, long, help = "subscribe indefinitely")]
    follow: bool,
    #[arg(
        short,
        long,
        value_name = "TOPIC",
        default_value = "",
        help = "topic name"
    )]
    topic: Topic,
    #[arg(value_name = "MESSAGE")]
    message: Option<String>,
}

#[derive(ClapArgs)]
#[group(required = true, multiple = false)]
struct ModeArgs {
    #[arg(short, long, help = "publish to a pipe")]
    publish: bool,
    #[arg(
        short,
        long,
        help = "subscribe to a pipe until one message is received"
    )]
    subscribe: bool,
}

impl From<Fanpipe> for Config {
    fn from(args: Fanpipe) -> Self {
        let mode = match args.mode {
            ModeArgs { publish: true, .. } => Mode::Publish(Message::resolve(args.message)),
            ModeArgs { .. } if args.follow => Mode::Subscribe(Until::Forever),
            ModeArgs { .. } => Mode::Subscribe(Until::FirstMessage),
        };
        Config {
            topic: args.topic,
            mode,
        }
    }
}
