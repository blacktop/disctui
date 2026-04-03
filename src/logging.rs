use std::fs::{File, OpenOptions};
use std::os::fd::AsRawFd;

use color_eyre::eyre::Result;
use tracing_subscriber::{EnvFilter, fmt, prelude::*};

/// Initialize file-based logging under the platform data directory.
///
/// Also redirects stderr to the log file so that `eprintln!` from
/// dependencies (like diself) does not corrupt the ratatui TUI.
pub fn init(debug: bool) -> Result<()> {
    let filter = if debug {
        EnvFilter::new("debug")
    } else {
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"))
    };

    let log_dir = dirs::state_dir()
        .or_else(dirs::data_local_dir)
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("disctui");

    std::fs::create_dir_all(&log_dir)?;

    let log_path = log_dir.join("disctui.log");

    // Open once, share the same fd for both stderr redirect and tracing.
    // Use create+truncate so each session starts fresh.
    let log_file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(&log_path)?;

    // Redirect stderr to the log file before ratatui takes over the terminal.
    redirect_stderr(&log_file)?;

    // Clone the handle for the tracing writer — same underlying fd, same offset.
    let tracing_writer = log_file
        .try_clone()
        .map_err(|e| color_eyre::eyre::eyre!("failed to clone log file handle: {e}"))?;

    tracing_subscriber::registry()
        .with(filter)
        .with(
            fmt::layer()
                .with_writer(tracing_writer)
                .with_ansi(false)
                .with_target(true),
        )
        .init();

    tracing::debug!("logging to {}", log_path.display());

    Ok(())
}

/// Redirect stderr (fd 2) to the given file using dup2.
fn redirect_stderr(file: &File) -> Result<()> {
    // SAFETY: dup2 is a standard POSIX call. We own the file descriptor.
    let result = unsafe { libc::dup2(file.as_raw_fd(), libc::STDERR_FILENO) };
    if result == -1 {
        return Err(color_eyre::eyre::eyre!(
            "failed to redirect stderr: {}",
            std::io::Error::last_os_error()
        ));
    }
    Ok(())
}
