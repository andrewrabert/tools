//! Port of `network-scan`: list the live hosts of a network with hostnames.
//!
//! Does the host discovery of `nmap -sn` itself: an ICMP echo to every address
//! through an unprivileged ping socket plus a TCP connect to the web ports,
//! then a reverse DNS lookup of whoever answered.

use std::collections::BTreeSet;
use std::io::{self, Write};
use std::net::{IpAddr, SocketAddr, TcpStream, ToSocketAddrs};
use std::num::NonZeroUsize;
use std::process::ExitCode;
use std::sync::Mutex;
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use clap::Args as ClapArgs;
use ipnet::IpNet;
use socket2::{Domain, Protocol, SockAddr, Socket, Type};

use crate::pool;
use crate::tools::Tool;

const TCP_PORTS: [u16; 2] = [80, 443];
const ECHO_ROUNDS: usize = 2;
const ROUND_GAP: Duration = Duration::from_millis(200);
const REPLY_TIMEOUT: Duration = Duration::from_secs(1);
const POLL: Duration = Duration::from_millis(100);
const TCP_WORKERS: usize = 128;

const ICMPV4_ECHO_REQUEST: u8 = 8;
const ICMPV4_ECHO_REPLY: u8 = 0;
const ICMPV6_ECHO_REQUEST: u8 = 128;
const ICMPV6_ECHO_REPLY: u8 = 129;

#[derive(ClapArgs)]
#[command(about = "List the live hosts of a network with their hostnames")]
pub struct NetworkScan {
    /// Network in CIDR notation, an address, or a hostname
    address: String,
}

impl Tool for NetworkScan {
    fn run(self) -> ExitCode {
        match run(&self.address) {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("Error: {error:?}");
                ExitCode::FAILURE
            }
        }
    }
}

fn run(target: &str) -> Result<()> {
    let addresses = resolve(target)?;
    let live = Mutex::new(BTreeSet::new());
    thread::scope(|scope| -> Result<()> {
        let icmp = scope.spawn(|| ping_all(&addresses, &live));
        let tcp = scope.spawn(|| connect_all(&addresses, &live));
        icmp.join().expect("ping thread panicked")?;
        tcp.join().expect("tcp thread panicked")
    })?;
    let live = live
        .into_inner()
        .unwrap_or_else(|poison| poison.into_inner());

    let hosts: Vec<(String, String)> = live
        .into_iter()
        .map(|address| (address.to_string(), hostname(address).unwrap_or_default()))
        .collect();
    print_table(&hosts)
}

fn resolve(target: &str) -> Result<Vec<IpAddr>> {
    if let Ok(net) = target.parse::<IpNet>() {
        return Ok(net.hosts().collect());
    }
    if let Ok(address) = target.parse::<IpAddr>() {
        return Ok(vec![address]);
    }
    let resolved = (target, 0)
        .to_socket_addrs()
        .with_context(|| format!("resolving {target}"))?
        .map(|socket| socket.ip())
        .collect::<Vec<_>>();
    if resolved.is_empty() {
        bail!("{target} resolved to no address");
    }
    Ok(resolved)
}

fn ping_all(addresses: &[IpAddr], live: &Mutex<BTreeSet<IpAddr>>) -> Result<()> {
    let v4: Vec<IpAddr> = addresses.iter().copied().filter(IpAddr::is_ipv4).collect();
    let v6: Vec<IpAddr> = addresses.iter().copied().filter(IpAddr::is_ipv6).collect();
    thread::scope(|scope| {
        let ping_v4 = scope.spawn(|| ping_family(Domain::IPV4, &v4, live));
        let ping_v6 = scope.spawn(|| ping_family(Domain::IPV6, &v6, live));
        ping_v4.join().expect("ipv4 ping thread panicked")?;
        ping_v6.join().expect("ipv6 ping thread panicked")
    })
}

fn ping_family(domain: Domain, addresses: &[IpAddr], live: &Mutex<BTreeSet<IpAddr>>) -> Result<()> {
    if addresses.is_empty() {
        return Ok(());
    }
    let (protocol, request, reply, name) = match domain {
        Domain::IPV6 => (
            Protocol::ICMPV6,
            ICMPV6_ECHO_REQUEST,
            ICMPV6_ECHO_REPLY,
            "ICMPv6",
        ),
        _ => (
            Protocol::ICMPV4,
            ICMPV4_ECHO_REQUEST,
            ICMPV4_ECHO_REPLY,
            "ICMP",
        ),
    };
    let socket = Socket::new(domain, Type::DGRAM, Some(protocol)).with_context(|| {
        format!("opening an unprivileged {name} socket (check net.ipv4.ping_group_range)")
    })?;
    socket.set_read_timeout(Some(POLL))?;

    let deadline = Mutex::new(None::<Instant>);
    thread::scope(|scope| -> Result<()> {
        let receiver = scope.spawn(|| receive(&socket, reply, &deadline, live));
        send_rounds(&socket, request, addresses);
        *deadline.lock().unwrap_or_else(|poison| poison.into_inner()) =
            Some(Instant::now() + REPLY_TIMEOUT);
        receiver.join().expect("receiver thread panicked")
    })
}

fn send_rounds(socket: &Socket, request: u8, addresses: &[IpAddr]) {
    for round in 0..ECHO_ROUNDS {
        if round > 0 {
            thread::sleep(ROUND_GAP);
        }
        for (index, address) in addresses.iter().enumerate() {
            let packet = echo_request(request, index as u16);
            let target = SockAddr::from(SocketAddr::new(*address, 0));
            // A send that fails (no route, host unreachable) just means the
            // address is not live.
            let _ = socket.send_to(&packet, &target);
        }
    }
}

/// An echo request carrying the sequence number.  Ping sockets fill in the
/// identifier and recompute the IPv4 checksum, so the checksum here only
/// needs to be well formed.
fn echo_request(kind: u8, sequence: u16) -> [u8; 8] {
    let [seq_hi, seq_lo] = sequence.to_be_bytes();
    let mut packet = [kind, 0, 0, 0, 0, 0, seq_hi, seq_lo];
    let checksum = internet_checksum(&packet);
    packet[2..4].copy_from_slice(&checksum.to_be_bytes());
    packet
}

fn internet_checksum(data: &[u8]) -> u16 {
    let mut sum: u32 = data
        .chunks(2)
        .map(|chunk| u32::from(u16::from_be_bytes([chunk[0], *chunk.get(1).unwrap_or(&0)])))
        .sum();
    while sum >> 16 != 0 {
        sum = (sum & 0xffff) + (sum >> 16);
    }
    !(sum as u16)
}

/// Collects echo replies until the deadline set by the sender passes.
fn receive(
    socket: &Socket,
    reply: u8,
    deadline: &Mutex<Option<Instant>>,
    live: &Mutex<BTreeSet<IpAddr>>,
) -> Result<()> {
    let mut buffer = [std::mem::MaybeUninit::<u8>::uninit(); 1500];
    loop {
        match socket.recv_from(&mut buffer) {
            Ok((len, source)) => {
                // SAFETY: recv_from initialised the first `len` bytes.
                let data = unsafe { std::slice::from_raw_parts(buffer.as_ptr().cast::<u8>(), len) };
                if let (Some(&kind), Some(source)) = (data.first(), source.as_socket())
                    && kind == reply
                {
                    live.lock()
                        .unwrap_or_else(|poison| poison.into_inner())
                        .insert(source.ip());
                }
            }
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                ) => {}
            Err(error) => return Err(error).context("receiving echo replies"),
        }
        let deadline = *deadline.lock().unwrap_or_else(|poison| poison.into_inner());
        if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
            return Ok(());
        }
    }
}

fn connect_all(addresses: &[IpAddr], live: &Mutex<BTreeSet<IpAddr>>) -> Result<()> {
    let jobs: Vec<SocketAddr> = addresses
        .iter()
        .flat_map(|address| TCP_PORTS.map(|port| SocketAddr::new(*address, port)))
        .collect();
    let workers = NonZeroUsize::new(TCP_WORKERS.min(jobs.len().max(1))).expect("at least one");
    pool::run(
        jobs,
        workers,
        |target| TcpStream::connect_timeout(target, REPLY_TIMEOUT).is_ok(),
        |target, connected| {
            if connected {
                live.lock()
                    .unwrap_or_else(|poison| poison.into_inner())
                    .insert(target.ip());
            }
            Ok(())
        },
    )
}

/// The reverse DNS name of an address, if it has one.
fn hostname(address: IpAddr) -> Option<String> {
    let name = dns_lookup::lookup_addr(&address).ok()?;
    // Without a PTR record the resolver hands the address back.
    (name != address.to_string()).then_some(name)
}

/// Columns padded to their widest value and joined by tabs, like the script.
fn print_table(rows: &[(String, String)]) -> Result<()> {
    let address_width = rows
        .iter()
        .map(|(address, _)| address.len())
        .max()
        .unwrap_or(0);
    let name_width = rows.iter().map(|(_, name)| name.len()).max().unwrap_or(0);
    let mut stdout = io::stdout().lock();
    for (address, name) in rows {
        writeln!(stdout, "{address:address_width$}\t{name:name_width$}")?;
    }
    stdout.flush()?;
    Ok(())
}
