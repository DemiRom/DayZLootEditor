use std::{
    env,
    io::{self, Read, Write},
    net::TcpStream,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use ssh2::{Session, Sftp};

#[derive(Clone)]
pub enum FileSource {
    Local,
    Remote(Arc<Mutex<SshBackend>>),
}

#[derive(Clone)]
pub struct FileSelection {
    pub path: PathBuf,
    pub source: FileSource,
}

#[derive(Clone, Debug)]
pub struct RemoteConfig {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub password: Option<String>,
    pub key_path: Option<PathBuf>,
    pub passphrase: Option<String>,
}

impl RemoteConfig {
    pub fn from_env() -> Option<Self> {
        let host = env::var("SSH_HOST").ok()?;
        let username = env::var("SSH_USER").ok()?;
        let port = env::var("SSH_PORT")
            .ok()
            .and_then(|p| p.parse().ok())
            .unwrap_or(22);
        let password = env::var("SSH_PASSWORD").ok();
        let key_path = env::var("SSH_KEY").ok().map(PathBuf::from);
        let passphrase = env::var("SSH_PASSPHRASE").ok();

        Some(Self {
            host,
            port,
            username,
            password,
            key_path,
            passphrase,
        })
    }
}

pub struct SshBackend {
    _session: Session,
    sftp: Sftp,
}

impl SshBackend {
    pub fn connect(config: &RemoteConfig) -> io::Result<Arc<Mutex<Self>>> {
        let tcp = TcpStream::connect((&*config.host, config.port))
            .map_err(|e| io::Error::new(io::ErrorKind::Other, format!("SSH connect: {e}")))?;
        let mut session = Session::new()
            .map_err(|e| io::Error::new(io::ErrorKind::Other, format!("Failed to create SSH session: {e}")))?;
        session.set_tcp_stream(tcp);
        session
            .handshake()
            .map_err(|e| io::Error::new(io::ErrorKind::Other, format!("SSH handshake: {e}")))?;

        if let Some(ref key) = config.key_path {
            session
                .userauth_pubkey_file(
                    &config.username,
                    None,
                    key,
                    config.passphrase.as_deref(),
                )
                .map_err(|e| io::Error::new(io::ErrorKind::Other, format!("SSH key auth: {e}")))?;
        } else if let Some(ref pwd) = config.password {
            session
                .userauth_password(&config.username, pwd)
                .map_err(|e| io::Error::new(io::ErrorKind::Other, format!("SSH password auth: {e}")))?;
        } else {
            session
                .userauth_agent(&config.username)
                .map_err(|e| io::Error::new(io::ErrorKind::Other, format!("SSH agent auth: {e}")))?;
        }

        if !session.authenticated() {
            return Err(io::Error::new(io::ErrorKind::Other, "SSH authentication failed"));
        }

        let sftp = session
            .sftp()
            .map_err(|e| io::Error::new(io::ErrorKind::Other, format!("SSH SFTP init: {e}")))?;

        Ok(Arc::new(Mutex::new(Self { _session: session, sftp })))
    }

    pub fn list_dir(&self, path: &Path) -> io::Result<Vec<DirEntry>> {
        let mut entries = Vec::new();
        for (p, stat) in self
            .sftp
            .readdir(path)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, format!("SFTP readdir: {e}")))? {
            let Some(name) = p.file_name().and_then(|n| n.to_str()).map(|s| s.to_string()) else {
                continue;
            };
            let is_dir = is_dir(&stat);
            entries.push(DirEntry { name, is_dir });
        }
        Ok(entries)
    }

    pub fn read_file(&self, path: &Path) -> io::Result<String> {
        let mut file = self
            .sftp
            .open(path)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, format!("SFTP open: {e}")))?;
        let mut buf = String::new();
        file.read_to_string(&mut buf)?;
        Ok(buf)
    }

    pub fn write_file(&self, path: &Path, contents: &str) -> io::Result<()> {
        let mut file = self
            .sftp
            .create(path)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, format!("SFTP create: {e}")))?;
        file.write_all(contents.as_bytes())?;
        Ok(())
    }
}

pub struct DirEntry {
    pub name: String,
    pub is_dir: bool,
}

const S_IFDIR: u32 = 0o040000;
const S_IFMT: u32 = 0o170000;

fn is_dir(stat: &ssh2::FileStat) -> bool {
    stat.perm
        .map(|p| p & S_IFMT == S_IFDIR)
        .unwrap_or(false)
}
