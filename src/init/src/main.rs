//! Nitro Enclave init process for chat-relayer — PID 1 inside the initramfs.
//!
//! Sequence:
//!   1. Mount pseudo-filesystems (proc, sys, dev, …)
//!   2. Reopen stdio on /dev/console
//!   3. aws::init_platform (NSM heartbeat → nitro-cli ready signal, insmod nsm.ko)
//!   4. Seed the kernel entropy pool
//!   5. Bring up loopback + per-egress-host loopback aliases
//!   6. Wait for the parent to push config over VSOCK:7000 (KEY=VALUE lines)
//!   7. Write /etc/hosts overrides + start socat VSOCK↔TCP bridges
//!   8. Spawn the chat_relayer binary; tail its log file to parent VSOCK:5000
//!
//! Unlike the coordinator, chat-relayer has no libp2p mesh and no TS
//! sidecar — it's a single HTTP server with several outbound HTTPS
//! dependencies (Postgres, Redis, Sui RPC, the embedding API, two
//! Walrus endpoints, and the coordinator/pinaivu-api itself). Each
//! outbound host gets its own loopback alias + VSOCK egress port,
//! since a single bridge can't multiplex multiple TLS-SNI hosts on
//! the same local port.

use std::{
    fs::{File, OpenOptions},
    io::BufReader,
    os::unix::io::FromRawFd,
    process::Command,
};

use aws::{get_entropy, init_platform};
use system::{dmesg, freopen, mount, reboot, seed_entropy, vsock_accept};

mod config;

/// CID of the parent partition — always 3 in the Nitro Enclave spec.
const PARENT_CID: u32 = 3;

/// One outbound HTTPS/TCP dependency: an env var naming the URL, the
/// loopback alias it gets mapped to, and the VSOCK port the parent
/// forwards to the real host.
struct Egress {
    env_var: &'static str,
    alias_ip: &'static str,
    vsock_port: u32,
    default_port: u16,
}

const EGRESS: &[Egress] = &[
    Egress { env_var: "SUI_RPC_URL", alias_ip: "127.0.0.11", vsock_port: 8103, default_port: 443 },
    Egress { env_var: "EMBEDDING_API_BASE", alias_ip: "127.0.0.12", vsock_port: 8104, default_port: 443 },
    Egress { env_var: "WALRUS_PUBLISHER_URL", alias_ip: "127.0.0.13", vsock_port: 8105, default_port: 443 },
    Egress { env_var: "WALRUS_AGGREGATOR_URL", alias_ip: "127.0.0.14", vsock_port: 8106, default_port: 443 },
    Egress { env_var: "PINAIVU_API_BASE", alias_ip: "127.0.0.15", vsock_port: 8107, default_port: 443 },
];

fn init_rootfs() {
    use libc::{MS_NODEV, MS_NOEXEC, MS_NOSUID};
    let no_dse = MS_NODEV | MS_NOSUID | MS_NOEXEC;
    let no_se = MS_NOSUID | MS_NOEXEC;

    let mounts = [
        ("devtmpfs", "/dev", "devtmpfs", no_se, "mode=0755"),
        ("devpts", "/dev/pts", "devpts", no_se, ""),
        ("shm", "/dev/shm", "tmpfs", no_dse, "mode=0755"),
        ("proc", "/proc", "proc", no_dse, "hidepid=2"),
        ("tmpfs", "/run", "tmpfs", no_dse, "mode=0755"),
        ("tmpfs", "/tmp", "tmpfs", no_dse, ""),
        ("sysfs", "/sys", "sysfs", no_dse, ""),
        ("cgroup_root", "/sys/fs/cgroup", "tmpfs", no_dse, "mode=0755"),
    ];

    for (src, target, fstype, flags, data) in mounts {
        let _ = std::fs::create_dir_all(target);
        match mount(src, target, fstype, flags, data) {
            Ok(()) => dmesg(format!("mounted {target}")),
            Err(e) => eprintln!("mount {target}: {e}"),
        }
    }
}

fn init_console() {
    for (path, mode, fd) in [
        ("/dev/console", "r", 0),
        ("/dev/console", "w", 1),
        ("/dev/console", "w", 2),
    ] {
        if let Err(e) = freopen(path, mode, fd) {
            eprintln!("freopen {path}: {e}");
        }
    }
}

/// Bring up loopback plus one alias address per egress host. Without
/// this, only 127.0.0.1 exists and a single port can't serve more
/// than one TLS-SNI-distinct host.
fn setup_loopback() {
    let _ = Command::new("/bin/busybox")
        .args(["ip", "addr", "add", "127.0.0.1/8", "dev", "lo"])
        .status();
    for eg in EGRESS {
        let _ = Command::new("/bin/busybox")
            .args(["ip", "addr", "add", &format!("{}/8", eg.alias_ip), "dev", "lo"])
            .status();
    }
    let _ = Command::new("/bin/busybox")
        .args(["ip", "link", "set", "dev", "lo", "up"])
        .status();
    dmesg("loopback + egress aliases up".into());
}

fn bridge(left: &str, right: &str) {
    match Command::new("/socat").arg(left).arg(right).spawn() {
        Ok(_) => dmesg(format!("bridge {left} <-> {right}")),
        Err(e) => eprintln!("socat {left} {right}: {e}"),
    }
}

/// Parse `scheme://host[:port]/...` into `(host, port)`, falling back
/// to `default_port` when the URL has none.
fn host_port(url: &str, default_port: u16) -> Option<(String, u16)> {
    let rest = url.strip_prefix("https://").or_else(|| url.strip_prefix("http://"))?;
    let authority = rest.split('/').next()?;
    let mut parts = authority.rsplitn(2, ':');
    let maybe_port = parts.next()?;
    match parts.next() {
        Some(host) if maybe_port.chars().all(|c| c.is_ascii_digit()) && !maybe_port.is_empty() => {
            Some((host.to_string(), maybe_port.parse().unwrap_or(default_port)))
        }
        _ => Some((authority.to_string(), default_port)),
    }
}

/// Tail a file and stream every new byte to a VSOCK peer (see coordinator's
/// init for the rationale on raw `libc::send` over std::fs::File writes).
fn log_forwarder(path: &str, cid: u32, port: u32) {
    use std::io::{Read, Seek, SeekFrom};
    use std::time::Duration;

    let mut pos: u64 = 0;
    let mut buf = [0u8; 4096];

    'outer: loop {
        let sock_fd = match vsock_connect(cid, port) {
            Ok(fd) => fd,
            Err(_) => {
                std::thread::sleep(Duration::from_secs(1));
                continue;
            }
        };

        loop {
            let mut file = match std::fs::File::open(path) {
                Ok(f) => f,
                Err(_) => {
                    std::thread::sleep(Duration::from_millis(200));
                    continue;
                }
            };
            let size = file.metadata().map(|m| m.len()).unwrap_or(0);
            if size < pos {
                pos = 0;
            }
            if size <= pos {
                std::thread::sleep(Duration::from_millis(100));
                continue;
            }
            if file.seek(SeekFrom::Start(pos)).is_err() {
                pos = 0;
                continue;
            }
            let n = match file.read(&mut buf) {
                Ok(0) => {
                    std::thread::sleep(Duration::from_millis(100));
                    continue;
                }
                Ok(n) => n,
                Err(_) => {
                    std::thread::sleep(Duration::from_millis(100));
                    continue;
                }
            };

            let mut sent = 0usize;
            while sent < n {
                let rc = unsafe {
                    libc::send(
                        sock_fd,
                        buf[sent..n].as_ptr() as *const libc::c_void,
                        n - sent,
                        libc::MSG_NOSIGNAL,
                    )
                };
                if rc < 0 {
                    unsafe { libc::close(sock_fd) };
                    std::thread::sleep(Duration::from_millis(200));
                    continue 'outer;
                }
                sent += rc as usize;
            }
            pos += n as u64;
        }
    }
}

fn vsock_connect(cid: u32, port: u32) -> Result<i32, String> {
    use libc::{connect, sockaddr, sockaddr_vm, socket, AF_VSOCK, SOCK_STREAM};
    let fd = unsafe { socket(AF_VSOCK, SOCK_STREAM, 0) };
    if fd < 0 {
        return Err("vsock socket() failed".into());
    }
    let ret = unsafe {
        let mut sa: sockaddr_vm = std::mem::zeroed();
        sa.svm_family = AF_VSOCK as _;
        sa.svm_port = port;
        sa.svm_cid = cid;
        connect(
            fd,
            &sa as *const _ as *const sockaddr,
            std::mem::size_of::<sockaddr_vm>() as _,
        )
    };
    if ret < 0 {
        unsafe { libc::close(fd) };
        Err(format!("vsock connect({cid}, {port}) failed"))
    } else {
        Ok(fd)
    }
}

fn main() {
    init_rootfs();
    init_console();
    init_platform();

    match seed_entropy(4096, get_entropy) {
        Ok(n) => dmesg(format!("entropy seeded: {n} bytes")),
        Err(e) => eprintln!("entropy: {e}"),
    }

    setup_loopback();
    dmesg("chat-relayer enclave booted".into());

    // ── Config injection via VSOCK:7000 ──────────────────────────────────────
    match vsock_accept(7000) {
        Ok(fd) => {
            let reader = BufReader::new(unsafe { File::from_raw_fd(fd) });
            let pairs = config::read_config(reader);
            for (k, v) in &pairs {
                std::env::set_var(k, v);
            }
            dmesg(format!("config injected: {} vars", pairs.len()));
        }
        Err(e) => eprintln!("config vsock: {e}"),
    }

    // ── Env var defaults ──────────────────────────────────────────────────────
    std::env::set_var("SSL_CERT_FILE", "/etc/ssl/certs/ca-certificates.crt");
    std::env::set_var("PATH", "/bin:/sbin:/usr/bin:/usr/sbin:/");
    if std::env::var("CHAT_RELAYER_BIND").is_err() {
        std::env::set_var("CHAT_RELAYER_BIND", "127.0.0.1:4002");
    }
    if std::env::var("DATABASE_URL").is_err() {
        std::env::set_var("DATABASE_URL", "postgresql://chatrelayer@127.0.0.1:5432/chatrelayer");
    }
    if std::env::var("REDIS_URL").is_err() {
        std::env::set_var("REDIS_URL", "redis://127.0.0.1:6379");
    }

    // ── /etc/hosts overrides + outbound bridges ──────────────────────────────
    // Each egress host's real hostname is mapped to its own loopback alias
    // (not 127.0.0.1) so simultaneous HTTPS connections to different hosts
    // on port 443 don't collide on the same local listener.
    let mut hosts = String::from("127.0.0.1 localhost\n");
    for var in ["POSTGRES_BRIDGE_HOST", "REDIS_BRIDGE_HOST"] {
        if let Ok(h) = std::env::var(var) {
            let h = h.trim();
            if !h.is_empty() {
                hosts.push_str(&format!("127.0.0.1 {h}\n"));
                dmesg(format!("/etc/hosts: 127.0.0.1 -> {h}"));
            }
        }
    }
    for eg in EGRESS {
        let Ok(url) = std::env::var(eg.env_var) else { continue };
        let Some((host, port)) = host_port(&url, eg.default_port) else { continue };
        hosts.push_str(&format!("{} {}\n", eg.alias_ip, host));
        dmesg(format!("/etc/hosts: {} -> {}", eg.alias_ip, host));
        bridge(
            &format!("TCP-LISTEN:{},bind={},reuseaddr,fork", port, eg.alias_ip),
            &format!("VSOCK-CONNECT:{PARENT_CID}:{}", eg.vsock_port),
        );
    }
    let _ = std::fs::write("/etc/hosts", hosts);

    // Postgres / Redis: same fixed-port pattern as the coordinator.
    bridge(
        "TCP-LISTEN:5432,reuseaddr,fork",
        &format!("VSOCK-CONNECT:{PARENT_CID}:8101"),
    );
    bridge(
        "TCP-LISTEN:6379,reuseaddr,fork",
        &format!("VSOCK-CONNECT:{PARENT_CID}:8102"),
    );

    // Inbound: parent delivers HTTP traffic via VSOCK:4002.
    bridge("VSOCK-LISTEN:4002,reuseaddr,fork", "TCP:127.0.0.1:4002");

    // ── Spawn chat_relayer ────────────────────────────────────────────────────
    let _ = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open("/tmp/chat-relayer.log");

    let log = OpenOptions::new()
        .create(true)
        .append(true)
        .open("/tmp/chat-relayer.log")
        .expect("open chat-relayer log");
    let log2 = log.try_clone().expect("clone log fd");

    dmesg("starting chat_relayer".into());
    let mut child = Command::new("/chat_relayer")
        .stdout(log)
        .stderr(log2)
        .spawn()
        .expect("failed to spawn chat_relayer");

    std::thread::spawn(|| log_forwarder("/tmp/chat-relayer.log", PARENT_CID, 5000));

    match child.wait() {
        Ok(s) => dmesg(format!("chat_relayer exited: {s}")),
        Err(e) => eprintln!("wait: {e}"),
    }

    std::thread::sleep(std::time::Duration::from_secs(5));
    reboot();
}
