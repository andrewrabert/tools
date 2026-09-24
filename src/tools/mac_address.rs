//! Generate a random MAC address.

use std::fmt;
use std::process::ExitCode;

use clap::Args as ClapArgs;

use crate::tools::Tool;

#[derive(ClapArgs)]
#[command(about = "Print a random locally administered MAC address")]
pub struct MacAddress {}

impl Tool for MacAddress {
    fn run(self) -> ExitCode {
        println!("{}", Address::random());
        ExitCode::SUCCESS
    }
}

/// A unicast, locally administered MAC address.
struct Address([u8; 6]);

impl Address {
    const MULTICAST: u8 = 0b0000_0001;
    const LOCALLY_ADMINISTERED: u8 = 0b0000_0010;

    fn random() -> Self {
        let mut octets: [u8; 6] = rand::random();
        octets[0] = (octets[0] & !Self::MULTICAST) | Self::LOCALLY_ADMINISTERED;
        Self(octets)
    }
}

impl fmt::Display for Address {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let [a, b, c, d, e, g] = self.0;
        write!(f, "{a:02x}:{b:02x}:{c:02x}:{d:02x}:{e:02x}:{g:02x}")
    }
}
