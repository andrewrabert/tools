mod cli;
mod config;
mod pipe;
mod topic;

use std::fs::{File, OpenOptions};
use std::io::{self, Read, Write};
use std::process::ExitCode;

use anyhow::{Context, Result};

use crate::tools::Tool;
pub use crate::tools::fanpipe::cli::Fanpipe;
use crate::tools::fanpipe::config::{Config, Mode};
pub use crate::tools::fanpipe::config::{Message, Until};
use crate::tools::fanpipe::pipe::PipeDir;
pub use crate::tools::fanpipe::topic::Topic;

const CHUNK_SIZE: usize = 64 * 1024;

impl Tool for Fanpipe {
    fn run(self) -> ExitCode {
        match run(self) {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("Error: {error:?}");
                ExitCode::FAILURE
            }
        }
    }
}

fn run(args: Fanpipe) -> Result<()> {
    let config = Config::from(args);
    match config.mode {
        Mode::Publish(message) => publish(&config.topic, message),
        Mode::Subscribe(until) => subscribe(&config.topic, until, &mut io::stdout().lock()),
    }
}

pub fn publish(topic: &Topic, message: Message) -> Result<()> {
    let dir = PipeDir::open();
    dir.sweep()?;
    let mut pipes = Vec::new();
    for path in dir.subscribers(topic)? {
        let pipe = OpenOptions::new()
            .write(true)
            .open(&path)
            .with_context(|| format!("opening {}", path.display()))?;
        pipes.push(pipe);
    }
    match message {
        Message::Text(text) => write_all(&mut pipes, text.as_bytes()),
        Message::Stdin => {
            let mut stdin = io::stdin().lock();
            let mut chunk = vec![0; CHUNK_SIZE];
            loop {
                let read = stdin.read(&mut chunk)?;
                if read == 0 {
                    return Ok(());
                }
                write_all(&mut pipes, &chunk[..read])?;
            }
        }
    }
}

pub fn subscribe(topic: &Topic, until: Until, out: &mut impl Write) -> Result<()> {
    let dir = PipeDir::open();
    dir.sweep()?;
    let pipe = dir.create(topic)?;
    loop {
        let mut reader = File::open(pipe.path())
            .with_context(|| format!("opening {}", pipe.path().display()))?;
        io::copy(&mut reader, out)?;
        out.flush()?;
        if let Until::FirstMessage = until {
            return Ok(());
        }
    }
}

fn write_all(pipes: &mut [File], bytes: &[u8]) -> Result<()> {
    for pipe in pipes {
        pipe.write_all(bytes)?;
    }
    Ok(())
}
