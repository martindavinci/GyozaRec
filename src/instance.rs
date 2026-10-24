//! Single instance: later launches forward their command to the first one.

use interprocess::local_socket::{GenericNamespaced, ListenerOptions, Name, Stream, prelude::*};
use std::io::{self, BufRead, BufReader, Write};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InstanceCommand {
    Activate,
    Capture,
}

impl InstanceCommand {
    pub fn as_line(self) -> &'static str {
        match self {
            InstanceCommand::Activate => "activate",
            InstanceCommand::Capture => "capture",
        }
    }

    pub fn parse(line: &str) -> Option<InstanceCommand> {
        match line.trim() {
            "activate" => Some(InstanceCommand::Activate),
            "capture" => Some(InstanceCommand::Capture),
            _ => None,
        }
    }
}

/// Per-user name (on Windows a named pipe, which is machine-wide).
pub fn socket_name(user: &str) -> String {
    format!("GyozaRec-{user}.sock")
}

pub struct InstanceListener(interprocess::local_socket::Listener);

impl InstanceListener {
    /// Handles one command line per connection on a background thread.
    pub fn spawn(self, on_command: impl Fn(InstanceCommand) + Send + 'static) {
        std::thread::Builder::new()
            .name("gyozarec-instance".into())
            .spawn(move || {
                for conn in self.0.incoming().filter_map(Result::ok) {
                    let mut line = String::new();
                    if BufReader::new(conn).read_line(&mut line).is_ok()
                        && let Some(cmd) = InstanceCommand::parse(&line)
                    {
                        on_command(cmd);
                    }
                }
            })
            .expect("spawn instance listener");
    }
}

pub struct InstanceClient(Stream);

impl InstanceClient {
    pub fn send(mut self, cmd: InstanceCommand) -> io::Result<()> {
        self.0.write_all(format!("{}\n", cmd.as_line()).as_bytes())?;
        self.0.flush()
    }
}

pub enum Instance {
    Primary(InstanceListener),
    Secondary(InstanceClient),
}

fn ns_name(name: &str) -> io::Result<Name<'_>> {
    name.to_ns_name::<GenericNamespaced>()
}

/// Connects to a running instance if there is one, otherwise starts listening.
pub fn acquire(name: &str) -> io::Result<Instance> {
    if let Ok(stream) = Stream::connect(ns_name(name)?) {
        return Ok(Instance::Secondary(InstanceClient(stream)));
    }
    match ListenerOptions::new().name(ns_name(name)?).create_sync() {
        Ok(listener) => Ok(Instance::Primary(InstanceListener(listener))),
        Err(err) if err.kind() == io::ErrorKind::AddrInUse => {
            Ok(Instance::Secondary(InstanceClient(Stream::connect(ns_name(name)?)?)))
        }
        Err(err) => Err(err),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn command_lines_roundtrip() {
        for c in [InstanceCommand::Activate, InstanceCommand::Capture] {
            assert_eq!(InstanceCommand::parse(&format!("{}\n", c.as_line())), Some(c));
        }
        assert_eq!(InstanceCommand::parse("boh"), None);
    }

    #[test]
    fn socket_name_is_per_user() {
        assert_eq!(socket_name("marty"), "GyozaRec-marty.sock");
    }

    #[test]
    fn second_instance_forwards_command_to_first() {
        let name = format!("GyozaRec-test-{}.sock", std::process::id());
        let Instance::Primary(listener) = acquire(&name).unwrap() else {
            panic!("first must be primary")
        };
        let (tx, rx) = std::sync::mpsc::channel();
        listener.spawn(move |c| {
            let _ = tx.send(c);
        });
        let Instance::Secondary(client) = acquire(&name).unwrap() else {
            panic!("second must be secondary")
        };
        client.send(InstanceCommand::Capture).unwrap();
        assert_eq!(rx.recv_timeout(Duration::from_secs(5)).unwrap(), InstanceCommand::Capture);
    }
}
