use std::io::{self, IsTerminal};

use crate::tools::fanpipe::topic::Topic;

pub enum Message {
    Text(String),
    Stdin,
}

impl Message {
    pub fn resolve(text: Option<String>) -> Self {
        if io::stdin().is_terminal() {
            Message::Text(text.unwrap_or_default())
        } else {
            Message::Stdin
        }
    }
}

pub enum Until {
    FirstMessage,
    Forever,
}

pub enum Mode {
    Publish(Message),
    Subscribe(Until),
}

pub struct Config {
    pub topic: Topic,
    pub mode: Mode,
}
