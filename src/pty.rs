use std::{
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    thread,
};

use portable_pty::{CommandBuilder, MasterPty, PtySize, native_pty_system};

pub struct PtyIo {
    pub writer: Arc<Mutex<Box<dyn Write + Send>>>,
    pub master: Arc<Mutex<Box<dyn MasterPty + Send>>>,
}

pub fn spawn_shell(
    cols: u16,
    rows: u16,
    cell_width: u32,
    cell_height: u32,
    output: flume::Sender<Vec<u8>>,
) -> Result<PtyIo, Box<dyn std::error::Error + Send + Sync>> {
    let system = native_pty_system();
    let pair = system.openpty(PtySize {
        rows,
        cols,
        pixel_width: cell_width as u16,
        pixel_height: cell_height as u16,
    })?;

    let shell = default_shell();
    let mut cmd = CommandBuilder::new(&shell);
    if is_powershell(&shell) {
        cmd.arg("-NoLogo");
    }
    cmd.env("TERM", "xterm-256color");
    cmd.env("COLORTERM", "truecolor");
    if let Some(home) = home_dir() {
        cmd.cwd(home);
    }

    let mut child = pair.slave.spawn_command(cmd)?;
    let mut reader = pair.master.try_clone_reader()?;
    let writer = pair.master.take_writer()?;
    let master = pair.master;

    thread::Builder::new().name("pty-reader".into()).spawn(move || {
        let mut buf = [0u8; 8192];
        loop {
            match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    if output.send(buf[..n].to_vec()).is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
        let _ = child.wait();
    })?;

    Ok(PtyIo {
        writer: Arc::new(Mutex::new(writer)),
        master: Arc::new(Mutex::new(master)),
    })
}

pub fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
}

fn default_shell() -> String {
    #[cfg(windows)]
    {
        windows_shell()
    }
    #[cfg(not(windows))]
    {
        std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".into())
    }
}

#[cfg(windows)]
fn windows_shell() -> String {
    if let Ok(shell) = std::env::var("SHELL") {
        if !shell.is_empty() && !shell.starts_with('/') {
            return shell;
        }
    }

    let system_root = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
    let powershell = PathBuf::from(&system_root)
        .join("System32")
        .join("WindowsPowerShell")
        .join("v1.0")
        .join("powershell.exe");
    if powershell.is_file() {
        return powershell.to_string_lossy().into_owned();
    }

    std::env::var("COMSPEC")
        .ok()
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| {
            PathBuf::from(system_root)
                .join("System32")
                .join("cmd.exe")
                .to_string_lossy()
                .into_owned()
        })
}

fn is_powershell(shell: &str) -> bool {
    Path::new(shell)
        .file_stem()
        .and_then(|stem| stem.to_str())
        .is_some_and(|stem| stem.eq_ignore_ascii_case("powershell") || stem.eq_ignore_ascii_case("pwsh"))
}

pub fn write_pty(writer: &Mutex<Box<dyn Write + Send>>, data: &[u8]) {
    if data.is_empty() {
        return;
    }
    if let Ok(mut writer) = writer.lock() {
        let _ = writer.write_all(data);
        let _ = writer.flush();
    }
}

pub fn resize_pty(master: &Mutex<Box<dyn MasterPty + Send>>, cols: u16, rows: u16, cw: u32, ch: u32) {
    if let Ok(master) = master.lock() {
        let _ = master.resize(PtySize {
            rows,
            cols,
            pixel_width: cw as u16,
            pixel_height: ch as u16,
        });
    }
}
