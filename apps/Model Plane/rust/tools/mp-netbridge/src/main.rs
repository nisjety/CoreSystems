//! `mp-netbridge` - the sandbox half of the egress boundary (EP-9 E3, D-E1).
//!
//! A tiered `code_interpreter` call runs in a sandbox whose network namespace
//! is unshared: it has a loopback interface and no route out. The one path out
//! is a Unix socket execution-core binds into the sandbox, behind which its
//! per-call CONNECT proxy admits only the tier's hosts. This binary is the
//! adapter in between: it listens on `127.0.0.1:3128` inside the namespace,
//! where `HTTPS_PROXY` points the program, forwards every connection byte for
//! byte to the socket, and runs the program as its child.
//!
//! It decides nothing. Which hosts are reachable is the proxy's decision, made
//! outside the sandbox where the program cannot touch it; this binary only
//! carries bytes, so a fault here can lose a connection but never widen one.
//!
//! Usage: `mp-netbridge <socket-path> -- <program> [args...]`. The exit code is
//! the program's, `128 + signal` when it died of one, and 125 when the bridge
//! itself could not start (the program is then never run).

use std::process::ExitCode;

/// Where the program's proxy variables point. Fixed: the sandbox's loopback is
/// its own, so nothing else can be listening there.
#[cfg_attr(not(unix), allow(dead_code))]
const LISTEN_ADDR: &str = "127.0.0.1:3128";

/// The bridge could not start; the program was not run.
const EXIT_BRIDGE_FAILED: u8 = 125;

/// A parsed command line.
#[derive(Debug, PartialEq, Eq)]
#[cfg_attr(not(unix), allow(dead_code))]
struct Invocation {
    socket: String,
    program: String,
    args: Vec<String>,
}

/// `<socket> -- <program> [args...]`, nothing else. The separator is required
/// so a program argument can never be read as the socket.
fn parse_args(raw: &[String]) -> Result<Invocation, String> {
    match raw {
        [socket, separator, program, args @ ..] if separator == "--" && !socket.is_empty() => {
            Ok(Invocation {
                socket: socket.clone(),
                program: program.clone(),
                args: args.to_vec(),
            })
        }
        _ => Err("usage: mp-netbridge <socket-path> -- <program> [args...]".to_owned()),
    }
}

fn main() -> ExitCode {
    let raw: Vec<String> = std::env::args().skip(1).collect();
    let invocation = match parse_args(&raw) {
        Ok(invocation) => invocation,
        Err(message) => {
            eprintln!("mp-netbridge: {message}");
            return ExitCode::from(EXIT_BRIDGE_FAILED);
        }
    };
    run(&invocation)
}

#[cfg(unix)]
fn run(invocation: &Invocation) -> ExitCode {
    use std::net::TcpListener;
    use std::os::unix::process::ExitStatusExt;

    let listener = match TcpListener::bind(LISTEN_ADDR) {
        Ok(listener) => listener,
        Err(error) => {
            eprintln!("mp-netbridge: cannot listen on {LISTEN_ADDR}: {error}");
            return ExitCode::from(EXIT_BRIDGE_FAILED);
        }
    };
    let socket = invocation.socket.clone();
    // Detached: the accept loop ends with the process, after the program exits.
    std::thread::spawn(move || accept_loop(&listener, &socket));

    let status = match std::process::Command::new(&invocation.program)
        .args(&invocation.args)
        .status()
    {
        Ok(status) => status,
        Err(error) => {
            eprintln!("mp-netbridge: cannot run {}: {error}", invocation.program);
            return ExitCode::from(EXIT_BRIDGE_FAILED);
        }
    };
    let code = status
        .code()
        .or_else(|| status.signal().map(|signal| 128 + signal))
        .unwrap_or(1);
    ExitCode::from(u8::try_from(code).unwrap_or(1))
}

#[cfg(not(unix))]
fn run(_invocation: &Invocation) -> ExitCode {
    eprintln!("mp-netbridge: Unix sockets are required; this host has none");
    ExitCode::from(EXIT_BRIDGE_FAILED)
}

/// Forward every accepted connection to the proxy socket. A connection the
/// socket refuses is closed, which the program sees as a failed proxy connect.
#[cfg(unix)]
fn accept_loop(listener: &std::net::TcpListener, socket: &str) {
    for stream in listener.incoming() {
        let Ok(tcp) = stream else { continue };
        let Ok(unix) = std::os::unix::net::UnixStream::connect(socket) else {
            continue;
        };
        std::thread::spawn(move || splice(tcp, unix));
    }
}

/// Copy both directions until each side closes, then shut the other side's
/// write half so a half-closed exchange (a request fully sent, a response still
/// coming) finishes instead of hanging.
#[cfg(unix)]
fn splice(tcp: std::net::TcpStream, unix: std::os::unix::net::UnixStream) {
    use std::net::Shutdown;

    let (Ok(mut tcp_read), Ok(mut unix_read)) = (tcp.try_clone(), unix.try_clone()) else {
        return;
    };
    let (mut tcp_write, mut unix_write) = (tcp, unix);
    let upstream = std::thread::spawn(move || {
        let _ = std::io::copy(&mut tcp_read, &mut unix_write);
        let _ = unix_write.shutdown(Shutdown::Write);
    });
    let _ = std::io::copy(&mut unix_read, &mut tcp_write);
    let _ = tcp_write.shutdown(Shutdown::Write);
    let _ = upstream.join();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    #[test]
    fn the_command_line_needs_a_socket_the_separator_and_a_program() {
        assert_eq!(
            parse_args(&strings(&["/run/egress.sock", "--", "python3", "main.py"])),
            Ok(Invocation {
                socket: "/run/egress.sock".to_owned(),
                program: "python3".to_owned(),
                args: strings(&["main.py"]),
            })
        );
        assert_eq!(
            parse_args(&strings(&["/run/egress.sock", "--", "sh"]))
                .expect("no program arguments is fine")
                .args,
            Vec::<String>::new()
        );
        for raw in [
            &[][..],
            &["/run/egress.sock"][..],
            &["/run/egress.sock", "--"][..],
            &["/run/egress.sock", "python3", "main.py"][..],
            &["", "--", "python3"][..],
            &["--", "python3"][..],
        ] {
            assert!(parse_args(&strings(raw)).is_err(), "{raw:?}");
        }
    }

    #[test]
    fn the_listen_address_is_loopback_on_the_proxy_port() {
        let address: std::net::SocketAddr = LISTEN_ADDR.parse().expect("a socket address");
        assert!(address.ip().is_loopback());
        assert_eq!(address.port(), 3128);
    }

    /// Bytes go both ways through the bridge, and a half-close finishes.
    #[cfg(unix)]
    #[test]
    fn a_connection_is_spliced_to_the_socket_both_ways() {
        use std::io::{Read, Write};
        use std::os::unix::net::UnixListener;

        let dir = std::env::temp_dir().join(format!("mp-netbridge-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("a test directory");
        let socket = dir.join("egress.sock");
        let proxy = UnixListener::bind(&socket).expect("a test socket");
        let echo = std::thread::spawn(move || {
            let (mut conn, _) = proxy.accept().expect("a bridged connection");
            let mut request = Vec::new();
            conn.read_to_end(&mut request).expect("the request");
            conn.write_all(b"reply:").expect("write");
            conn.write_all(&request).expect("write");
        });

        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("a loopback port");
        let address = listener.local_addr().expect("an address");
        let socket_path = socket.to_string_lossy().into_owned();
        std::thread::spawn(move || accept_loop(&listener, &socket_path));

        let mut client = std::net::TcpStream::connect(address).expect("connect");
        client.write_all(b"ping").expect("send");
        client
            .shutdown(std::net::Shutdown::Write)
            .expect("half-close");
        let mut reply = Vec::new();
        client.read_to_end(&mut reply).expect("the reply");
        assert_eq!(reply, b"reply:ping");
        echo.join().expect("the echo side");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
