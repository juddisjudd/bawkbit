use std::io::{BufRead, BufReader, ErrorKind, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::path::Path;
use std::time::{Duration, Instant};

const HELLO: &str = "bawkbit";

/// Magnet links, torrent links and .torrent files given on the command line, as when the OS opens one with bawkbit.
pub fn launch_args() -> Vec<String> {
    std::env::args()
        .skip(1)
        .filter_map(|arg| normalize(&arg))
        .collect()
}

fn normalize(arg: &str) -> Option<String> {
    let arg = arg.trim();
    let lower = arg.to_ascii_lowercase();
    if lower.starts_with("magnet:") || lower.starts_with("http://") || lower.starts_with("https://")
    {
        return Some(arg.to_string());
    }
    let path = Path::new(arg);
    path.is_file()
        .then(|| std::path::absolute(path).unwrap_or(path.to_path_buf()))
        .map(|path| path.to_string_lossy().into_owned())
}

pub enum Claim {
    /// Another bawkbit is running and now has the arguments.
    Forwarded,
    /// This is the only bawkbit; the listener hears from the next one started.
    Primary(Option<TcpListener>),
}

/// A port per data folder, so a second copy with its own BAWKBIT_HOME runs on its own.
fn port(home: &Path) -> u16 {
    let mut hash: u32 = 2166136261;
    for byte in home.to_string_lossy().to_lowercase().bytes() {
        hash = (hash ^ u32::from(byte)).wrapping_mul(16777619);
    }
    41000 + (hash % 9000) as u16
}

/// `patience` is how long a copy started by an update waits for the one it replaces to quit and free the port.
pub fn claim(home: &Path, args: &[String], patience: Duration) -> Claim {
    let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, port(home)));
    let deadline = Instant::now() + patience;
    while Instant::now() < deadline {
        match TcpListener::bind(addr) {
            Ok(listener) => return Claim::Primary(Some(listener)),
            Err(err) if err.kind() == ErrorKind::AddrInUse => {
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(_) => break,
        }
    }
    if let Ok(mut stream) = TcpStream::connect_timeout(&addr, Duration::from_millis(400)) {
        let mut message = format!("{HELLO}\n");
        for arg in args {
            message.push_str(arg);
            message.push('\n');
        }
        if stream.write_all(message.as_bytes()).is_ok() {
            return Claim::Forwarded;
        }
    }
    Claim::Primary(TcpListener::bind(addr).ok())
}

/// Hands each later launch's arguments to the window; an empty list means "come to the front".
pub fn listen(listener: TcpListener, sender: async_channel::Sender<Vec<String>>) {
    std::thread::Builder::new()
        .name("bawkbit-instance".into())
        .spawn(move || {
            for stream in listener.incoming().flatten() {
                let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
                let mut lines = BufReader::new(stream).lines();
                if lines.next().and_then(|line| line.ok()).as_deref() != Some(HELLO) {
                    continue;
                }
                let args: Vec<String> = lines
                    .map_while(|line| line.ok())
                    .filter_map(|line| normalize(&line))
                    .take(64)
                    .collect();
                if sender.send_blocking(args).is_err() {
                    break;
                }
            }
        })
        .expect("can start a thread");
}

#[cfg(windows)]
pub mod links {
    use windows_registry::CURRENT_USER;

    const CLASSES: &str = r"Software\Classes";

    fn command() -> Option<String> {
        let exe = std::env::current_exe().ok()?;
        Some(format!("\"{}\" \"%1\"", exe.display()))
    }

    /// True when magnet links open this copy of bawkbit.
    pub fn handled_here() -> bool {
        let key = format!(r"{CLASSES}\magnet\shell\open\command");
        let current = CURRENT_USER
            .open(&key)
            .ok()
            .and_then(|key| key.get_string("").ok());
        current.is_some_and(|current| {
            Some(current.to_lowercase()) == command().map(|c| c.to_lowercase())
        })
    }

    fn fail(err: impl std::fmt::Display) -> String {
        format!("cannot write to the registry: {err}")
    }

    /// Makes magnet links and .torrent files open with this copy, for this Windows user only.
    pub fn take_over() -> Result<(), String> {
        let command = command().ok_or("cannot find bawkbit's own path")?;
        let exe = std::env::current_exe().map_err(|err| err.to_string())?;
        let classes = CURRENT_USER.create(CLASSES).map_err(fail)?;

        let magnet = classes.create("magnet").map_err(fail)?;
        magnet.set_string("", "URL:Magnet link").map_err(fail)?;
        magnet.set_string("URL Protocol", "").map_err(fail)?;
        magnet
            .create(r"shell\open\command")
            .and_then(|key| key.set_string("", &command))
            .map_err(fail)?;

        let file = classes.create("bawkbit.torrent").map_err(fail)?;
        file.set_string("", "Torrent file").map_err(fail)?;
        file.create("DefaultIcon")
            .and_then(|key| key.set_string("", format!("\"{}\",0", exe.display())))
            .map_err(fail)?;
        file.create(r"shell\open\command")
            .and_then(|key| key.set_string("", &command))
            .map_err(fail)?;
        classes
            .create(".torrent")
            .and_then(|key| key.set_string("", "bawkbit.torrent"))
            .map_err(fail)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_only_torrent_arguments() {
        assert_eq!(
            normalize(" magnet:?xt=urn:btih:abc "),
            Some("magnet:?xt=urn:btih:abc".into())
        );
        assert_eq!(
            normalize("https://x/a.torrent"),
            Some("https://x/a.torrent".into())
        );
        assert_eq!(normalize("--flag"), None);
        assert_eq!(normalize("no/such/file.torrent"), None);
    }

    #[test]
    fn ports_differ_per_home() {
        let a = port(Path::new("C:/a"));
        assert_eq!(a, port(Path::new("c:/A")));
        assert_ne!(a, port(Path::new("C:/b")));
        assert!((41000..50000).contains(&a));
    }

    /// A data folder whose port this machine lets a test bind, as Windows reserves some port ranges.
    fn free_home(name: &str) -> std::path::PathBuf {
        (0..)
            .map(|n| {
                std::env::temp_dir().join(format!("bawkbit-{name}-{}-{n}", std::process::id()))
            })
            .find(|home| TcpListener::bind((Ipv4Addr::LOCALHOST, port(home))).is_ok())
            .unwrap()
    }

    #[test]
    fn a_second_launch_forwards_its_arguments() {
        let home = free_home("instance");
        let Claim::Primary(Some(listener)) = claim(&home, &[], Duration::ZERO) else {
            panic!("the first launch should listen");
        };
        let (sender, receiver) = async_channel::unbounded();
        listen(listener, sender);
        let link = "magnet:?xt=urn:btih:dd8255ecdc7ca55fb0bbf81323d87062db1f6d1c".to_string();
        assert!(matches!(
            claim(&home, std::slice::from_ref(&link), Duration::ZERO),
            Claim::Forwarded
        ));
        assert_eq!(receiver.recv_blocking().unwrap(), vec![link]);
    }

    #[test]
    fn a_launch_after_an_update_waits_for_the_old_copy_to_quit() {
        let home = free_home("update");
        let Claim::Primary(Some(old)) = claim(&home, &[], Duration::ZERO) else {
            panic!("the old copy should listen");
        };
        let started = Instant::now();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(300));
            drop(old);
        });
        let claimed = claim(&home, &[], Duration::from_secs(10));
        assert!(matches!(claimed, Claim::Primary(Some(_))));
        assert!(started.elapsed() >= Duration::from_millis(300));
    }
}
