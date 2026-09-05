use std::{
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    thread,
};

#[cfg(windows)]
use std::path::Path;

use portable_pty::{CommandBuilder, MasterPty, PtySize, native_pty_system};

pub struct PtyIo {
    pub writer: Arc<Mutex<Box<dyn Write + Send>>>,
    pub master: Arc<Mutex<Box<dyn MasterPty + Send>>>,
    pub pid: Option<u32>,
}

pub fn spawn_shell(
    cols: u16,
    rows: u16,
    cell_width: u32,
    cell_height: u32,
    output: flume::Sender<Vec<u8>>,
    cwd: Option<&Path>,
) -> Result<PtyIo, Box<dyn std::error::Error + Send + Sync>> {
    let system = native_pty_system();
    let pair = system.openpty(PtySize {
        rows,
        cols,
        pixel_width: cell_width as u16,
        pixel_height: cell_height as u16,
    })?;

    let shell = default_shell();
    let mut cmd = CommandBuilder::new(&shell.program);
    for arg in &shell.args {
        cmd.arg(arg);
    }
    cmd.env("TERM", "xterm-256color");
    cmd.env("COLORTERM", "truecolor");
    if let Some(path) = login_path() {
        cmd.env("PATH", path);
    }
    if let Some(cwd) = crate::cwd::usable_cwd(cwd).or_else(home_dir) {
        cmd.cwd(cwd);
    }

    let mut child = pair.slave.spawn_command(cmd)?;
    let pid = child.process_id();
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
        pid,
    })
}

pub fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
}

struct ShellLaunch {
    program: String,
    args: Vec<String>,
}

impl ShellLaunch {
    fn new(program: impl Into<String>) -> Self {
        Self {
            program: program.into(),
            args: Vec::new(),
        }
    }

    fn with_args(mut self, args: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.args.extend(args.into_iter().map(Into::into));
        self
    }
}

fn default_shell() -> ShellLaunch {
    #[cfg(windows)]
    {
        windows_shell()
    }
    #[cfg(not(windows))]
    {
        // Login so `.zprofile` / Homebrew `brew shellenv` run before `.zshrc`.
        // A Finder-launched .app does not inherit the PATH from iTerm.
        ShellLaunch::new(std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".into())).with_args(["-l"])
    }
}

fn login_path() -> Option<std::ffi::OsString> {
    let mut dirs = Vec::new();
    let mut push = |path: PathBuf| {
        if path.is_dir() && !dirs.iter().any(|existing| existing == &path) {
            dirs.push(path);
        }
    };
    for dir in login_path_dirs() {
        push(dir);
    }
    if let Some(existing) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&existing) {
            push(dir);
        }
    }
    std::env::join_paths(dirs).ok()
}

fn login_path_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    #[cfg(target_os = "macos")]
    {
        dirs.push(PathBuf::from("/opt/homebrew/bin"));
        dirs.push(PathBuf::from("/opt/homebrew/sbin"));
        dirs.push(PathBuf::from("/usr/local/bin"));
    }
    #[cfg(windows)]
    {
        if let Ok(local_app_data) = std::env::var("LOCALAPPDATA") {
            dirs.push(PathBuf::from(local_app_data).join("Programs").join("Git").join("cmd"));
        }
        if let Ok(program_files) = std::env::var("ProgramFiles") {
            dirs.push(PathBuf::from(program_files).join("Git").join("cmd"));
        }
    }
    if let Some(home) = home_dir() {
        dirs.push(home.join(".local").join("bin"));
        dirs.push(home.join(".cargo").join("bin"));
    }
    dirs
}

#[cfg(windows)]
fn windows_shell() -> ShellLaunch {
    if let Ok(shell) = std::env::var("SHELL") {
        if !shell.is_empty() && !shell.starts_with('/') {
            return launch_from_program(shell);
        }
    }

    if let Some(bash) = find_git_bash() {
        return git_bash_launch(bash);
    }

    let system_root = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
    let powershell = PathBuf::from(&system_root)
        .join("System32")
        .join("WindowsPowerShell")
        .join("v1.0")
        .join("powershell.exe");
    if powershell.is_file() {
        return powershell_launch(powershell);
    }

    let comspec = std::env::var("COMSPEC")
        .ok()
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| {
            PathBuf::from(system_root)
                .join("System32")
                .join("cmd.exe")
                .to_string_lossy()
                .into_owned()
        });
    ShellLaunch::new(comspec)
}

#[cfg(windows)]
fn launch_from_program(program: String) -> ShellLaunch {
    if is_powershell(&program) {
        return ShellLaunch::new(program).with_args(["-NoLogo"]);
    }
    if is_bash(&program) {
        return ShellLaunch::new(program).with_args(["-l", "-i"]);
    }
    ShellLaunch::new(program)
}

#[cfg(windows)]
fn git_bash_launch(path: PathBuf) -> ShellLaunch {
    ShellLaunch::new(path.to_string_lossy().into_owned()).with_args(["-l", "-i"])
}

#[cfg(windows)]
fn powershell_launch(path: PathBuf) -> ShellLaunch {
    ShellLaunch::new(path.to_string_lossy().into_owned()).with_args(["-NoLogo"])
}

/// `git-bash.exe` opens mintty in a separate window; the PTY host is `Git\bin\bash.exe`.
#[cfg(windows)]
fn find_git_bash() -> Option<PathBuf> {
    git_bash_candidates().into_iter().find(|path| path.is_file())
}

#[cfg(windows)]
fn git_bash_candidates() -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    let mut push_root = |root: PathBuf| {
        candidates.push(root.join("bin").join("bash.exe"));
    };

    if let Some(git) = find_on_path("git.exe") {
        if let Some(root) = git.parent().and_then(|cmd| cmd.parent()) {
            push_root(root.to_path_buf());
        }
    }
    if let Ok(program_files) = std::env::var("ProgramFiles") {
        push_root(PathBuf::from(program_files).join("Git"));
    }
    if let Ok(program_files_x86) = std::env::var("ProgramFiles(x86)") {
        push_root(PathBuf::from(program_files_x86).join("Git"));
    }
    if let Ok(local_app_data) = std::env::var("LOCALAPPDATA") {
        push_root(PathBuf::from(local_app_data).join("Programs").join("Git"));
    }
    if let Some(home) = home_dir() {
        push_root(home.join("scoop").join("apps").join("git").join("current"));
    }

    candidates
}

#[cfg(windows)]
fn find_on_path(file_name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path).find_map(|dir| {
        let candidate = dir.join(file_name);
        candidate.is_file().then_some(candidate)
    })
}

#[cfg(windows)]
fn is_powershell(program: &str) -> bool {
    file_stem_eq_ignore_ascii_case(program, &["powershell", "pwsh"])
}

#[cfg(windows)]
fn is_bash(program: &str) -> bool {
    file_stem_eq_ignore_ascii_case(program, &["bash"])
}

#[cfg(windows)]
fn file_stem_eq_ignore_ascii_case(program: &str, names: &[&str]) -> bool {
    Path::new(program)
        .file_stem()
        .and_then(|stem| stem.to_str())
        .is_some_and(|stem| names.iter().any(|name| stem.eq_ignore_ascii_case(name)))
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

#[cfg(test)]
mod tests {
    use super::login_path_dirs;

    #[test]
    fn login_path_includes_user_bin_dirs() {
        let dirs = login_path_dirs();
        if let Some(home) = super::home_dir() {
            assert!(dirs.contains(&home.join(".cargo").join("bin")));
        }
        #[cfg(target_os = "macos")]
        {
            assert!(dirs.iter().any(|path| path.ends_with("opt/homebrew/bin")));
        }
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
