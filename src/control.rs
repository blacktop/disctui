//! Local control socket. `disctui open <channel-id>` and tools such as Rex use it
//! to drive a TUI that is already running.

use std::fs::{File, TryLockError};
use std::io::ErrorKind;
use std::os::unix::fs::{FileTypeExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::time::Duration;

use color_eyre::eyre::{Result, WrapErr, eyre};
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::mpsc;

use crate::action::Action;

const MAX_REQUEST_BYTES: u64 = 4096;
const IO_TIMEOUT: Duration = Duration::from_secs(5);
const REPLY_OK: &str = "ok";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum Request {
    Open { channel_id: String },
}

pub fn socket_path() -> PathBuf {
    dirs::cache_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("disctui")
        .join("control.sock")
}

type SocketFile = (PathBuf, u64, u64);

/// Removes the socket file when the TUI exits.
pub struct SocketGuard {
    _listener: tokio::task::JoinSet<()>,
    // Drop in field order: unlink while the lock is still held.
    _socket: std::mem::DropGuard<SocketFile, fn(SocketFile)>,
    _lock: File,
}

fn remove_socket((path, dev, ino): SocketFile) {
    if std::fs::symlink_metadata(&path).is_ok_and(|m| (m.dev(), m.ino()) == (dev, ino))
        && let Err(err) = std::fs::remove_file(&path)
    {
        tracing::debug!("failed to remove control socket {}: {err}", path.display());
    }
}

/// Start accepting control requests. All instances arbitrate on the same
/// persistent lock file, including during stale-socket recovery and shutdown.
pub async fn listen(path: PathBuf, tx: mpsc::Sender<Action>) -> Result<Option<SocketGuard>> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).wrap_err("failed to create control socket directory")?;
    }
    let lock = File::options()
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path.with_extension("lock"))?;
    match lock.try_lock() {
        Ok(()) => {}
        Err(TryLockError::WouldBlock) => return Ok(None),
        Err(TryLockError::Error(err)) => return Err(err.into()),
    }
    let listener = match UnixListener::bind(&path) {
        Ok(listener) => listener,
        Err(err) if err.kind() == ErrorKind::AddrInUse => {
            let meta = std::fs::symlink_metadata(&path)?;
            if !meta.file_type().is_socket() {
                return Err(eyre!("control socket path is occupied by a non-socket"));
            }
            // An older running instance may predate the lock file.
            match tokio::time::timeout(IO_TIMEOUT, UnixStream::connect(&path)).await? {
                Ok(_) => return Ok(None),
                Err(e) if e.kind() == ErrorKind::ConnectionRefused => {}
                Err(e) => return Err(e.into()),
            }
            let current = std::fs::symlink_metadata(&path)?;
            if (meta.dev(), meta.ino()) != (current.dev(), current.ino()) {
                return Err(eyre!("control socket changed during startup"));
            }
            std::fs::remove_file(&path)?;
            UnixListener::bind(&path)?
        }
        Err(err) => return Err(err).wrap_err("failed to bind control socket"),
    };
    let meta = std::fs::symlink_metadata(&path)?;
    let socket = std::mem::DropGuard::new(
        (path.clone(), meta.dev(), meta.ino()),
        remove_socket as fn(_),
    );
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
    let mut tasks = tokio::task::JoinSet::new();
    tasks.spawn(async move {
        loop {
            match listener.accept().await {
                Ok((stream, _)) => {
                    let tx = tx.clone();
                    tokio::spawn(async move {
                        if let Err(err) = serve(stream, &tx).await {
                            tracing::warn!("control request failed: {err}");
                        }
                    });
                }
                Err(err) => {
                    tracing::warn!("control socket accept failed: {err}");
                    return;
                }
            }
        }
    });

    Ok(Some(SocketGuard {
        _listener: tasks,
        _socket: socket,
        _lock: lock,
    }))
}

async fn serve(stream: UnixStream, tx: &mpsc::Sender<Action>) -> Result<()> {
    let (reader, mut writer) = stream.into_split();
    let mut line = String::new();
    let mut reader = BufReader::new(reader.take(MAX_REQUEST_BYTES));
    tokio::time::timeout(IO_TIMEOUT, reader.read_line(&mut line))
        .await
        .wrap_err("timed out reading control request")??;

    let reply = match serde_json::from_str::<Request>(line.trim()) {
        Ok(Request::Open { channel_id }) => match tx.send(Action::OpenChannel(channel_id)).await {
            Ok(()) => REPLY_OK.to_string(),
            Err(_) => "error: disctui is shutting down".to_string(),
        },
        Err(err) => format!("error: invalid request: {err}"),
    };
    writer.write_all(format!("{reply}\n").as_bytes()).await?;
    Ok(())
}

/// Send one request to a running TUI.
pub async fn send(path: &Path, request: &Request) -> Result<()> {
    let mut stream = UnixStream::connect(path).await.map_err(|err| {
        eyre!("disctui is not running ({err}). Start it with `disctui --open <channel-id>` instead")
    })?;
    let payload = serde_json::to_string(request)?;
    stream.write_all(format!("{payload}\n").as_bytes()).await?;

    let mut reply = String::new();
    tokio::time::timeout(
        IO_TIMEOUT,
        BufReader::new(&mut stream).read_line(&mut reply),
    )
    .await
    .wrap_err("timed out waiting for disctui to answer")??;

    match reply.trim() {
        REPLY_OK => Ok(()),
        other => Err(eyre!("disctui refused the request: {other}")),
    }
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "unwrap is fine in tests")]
mod tests {
    use crate::control::*;

    fn temp_socket(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("disctui-test-{}-{name}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("control.sock")
    }

    #[test]
    fn open_request_uses_a_tagged_json_shape() {
        let request = Request::Open {
            channel_id: "123".into(),
        };
        assert_eq!(
            serde_json::to_string(&request).unwrap(),
            r#"{"cmd":"open","channel_id":"123"}"#
        );
    }

    #[tokio::test]
    async fn open_request_reaches_the_app_as_an_action() {
        let path = temp_socket("open");
        let (tx, mut rx) = mpsc::channel(4);
        let guard = listen(path.clone(), tx).await.unwrap();
        assert!(guard.is_some());

        send(
            &path,
            &Request::Open {
                channel_id: "c20".into(),
            },
        )
        .await
        .unwrap();

        let Some(Action::OpenChannel(channel_id)) = rx.recv().await else {
            unreachable!("expected an OpenChannel action");
        };
        assert_eq!(channel_id, "c20");

        drop(guard);
        assert!(!path.exists(), "socket file should be removed on drop");
    }

    #[tokio::test]
    async fn malformed_request_is_refused() {
        let path = temp_socket("malformed");
        let (tx, mut rx) = mpsc::channel(4);
        let _guard = listen(path.clone(), tx).await.unwrap();

        let mut stream = UnixStream::connect(&path).await.unwrap();
        stream.write_all(b"{\"cmd\":\"explode\"}\n").await.unwrap();
        let mut reply = String::new();
        BufReader::new(&mut stream)
            .read_line(&mut reply)
            .await
            .unwrap();

        assert!(reply.starts_with("error: invalid request"), "{reply}");
        assert!(rx.try_recv().is_err());
    }

    #[tokio::test]
    async fn send_without_a_listener_says_disctui_is_not_running() {
        let path = temp_socket("absent");
        let err = send(
            &path,
            &Request::Open {
                channel_id: "1".into(),
            },
        )
        .await
        .unwrap_err()
        .to_string();
        assert!(err.contains("not running"), "{err}");
    }

    #[tokio::test]
    async fn second_listener_leaves_the_first_in_charge() {
        let path = temp_socket("second");
        let (tx, _rx) = mpsc::channel(4);
        let (first, second) =
            tokio::join!(listen(path.clone(), tx.clone()), listen(path.clone(), tx));
        let owners = [first.unwrap(), second.unwrap()];
        assert_eq!(owners.iter().flatten().count(), 1);
        assert!(path.exists());
    }

    #[tokio::test]
    async fn contender_yields_before_the_owner_has_bound() {
        let path = temp_socket("starting");
        let lock = File::create(path.with_extension("lock")).unwrap();
        lock.try_lock().unwrap();
        let (tx, _rx) = mpsc::channel(1);
        assert!(listen(path, tx).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn occupied_regular_file_is_preserved() {
        let path = temp_socket("regular");
        std::fs::write(&path, b"owner data").unwrap();
        let (tx, _rx) = mpsc::channel(1);
        assert!(listen(path.clone(), tx).await.is_err());
        assert_eq!(std::fs::read(path).unwrap(), b"owner data");
    }

    #[tokio::test]
    async fn dropping_old_guard_preserves_replacement() {
        let path = temp_socket("replacement");
        let (tx, _rx) = mpsc::channel(1);
        let guard = listen(path.clone(), tx).await.unwrap().unwrap();
        std::fs::remove_file(&path).unwrap();
        let _replacement = UnixListener::bind(&path).unwrap();
        drop(guard);
        assert!(UnixStream::connect(path).await.is_ok());
    }

    #[tokio::test]
    async fn stale_socket_file_is_replaced() {
        let path = temp_socket("stale");
        drop(UnixListener::bind(&path).unwrap());
        let (tx, _rx) = mpsc::channel(4);
        assert!(listen(path, tx).await.unwrap().is_some());
    }
}
