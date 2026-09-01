use std::{
    io::{Read, Write},
    sync::{Arc, Mutex},
    thread,
};

use portable_pty::{native_pty_system, CommandBuilder, MasterPty, PtySize};

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

    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".into());
    let mut cmd = CommandBuilder::new(&shell);
    cmd.env("TERM", "xterm-256color");
    cmd.env("COLORTERM", "truecolor");
    if let Ok(home) = std::env::var("HOME") {
        cmd.cwd(home);
    }

    let mut child = pair.slave.spawn_command(cmd)?;
    let mut reader = pair.master.try_clone_reader()?;
    let writer = pair.master.take_writer()?;
    let master = pair.master;

    thread::Builder::new()
        .name("pty-reader".into())
        .spawn(move || {
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

pub fn write_pty(writer: &Mutex<Box<dyn Write + Send>>, data: &[u8]) {
    if data.is_empty() {
        return;
    }
    if let Ok(mut writer) = writer.lock() {
        let _ = writer.write_all(data);
        let _ = writer.flush();
    }
}

pub fn resize_pty(
    master: &Mutex<Box<dyn MasterPty + Send>>,
    cols: u16,
    rows: u16,
    cw: u32,
    ch: u32,
) {
    if let Ok(master) = master.lock() {
        let _ = master.resize(PtySize {
            rows,
            cols,
            pixel_width: cw as u16,
            pixel_height: ch as u16,
        });
    }
}
