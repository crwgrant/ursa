use std::path::{Path, PathBuf};

/// Working directory reported by OSC 7 or the local PTY process.
///
/// A remote host is kept so we never treat that path as a folder on this machine.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TerminalCwd {
    pub path: PathBuf,
    host: Option<String>,
}

impl TerminalCwd {
    pub fn local(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            host: None,
        }
    }

    pub fn remote(host: impl Into<String>, path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            host: Some(host.into()),
        }
    }

    pub fn is_remote(&self) -> bool {
        self.host.is_some()
    }
}

pub fn parse_pwd(raw: &str) -> Option<TerminalCwd> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    if let Some(rest) = raw.strip_prefix("file://") {
        return parse_file_authority_path(rest);
    }
    if let Some(rest) = raw.strip_prefix("file:") {
        return local_path(percent_decode(rest));
    }
    local_path(raw.to_string())
}

fn parse_file_authority_path(rest: &str) -> Option<TerminalCwd> {
    if rest.starts_with('/') {
        return local_path(percent_decode(rest));
    }
    let slash = rest.find('/')?;
    let host = authority_host(&percent_decode(&rest[..slash]));
    let path = percent_decode(&rest[slash..]);
    if is_local_host(&host) {
        local_path(path)
    } else {
        let path = normalize_file_path(path);
        if path.is_empty() {
            return None;
        }
        Some(TerminalCwd::remote(host, PathBuf::from(path)))
    }
}

fn local_path(path: String) -> Option<TerminalCwd> {
    let path = normalize_file_path(path);
    if path.is_empty() {
        return None;
    }
    Some(TerminalCwd::local(PathBuf::from(path)))
}

fn authority_host(auth: &str) -> String {
    let auth = auth.trim();
    if let Some(rest) = auth.strip_prefix('[') {
        if let Some(end) = rest.find(']') {
            return rest[..end].to_string();
        }
    }
    let host = auth.rsplit_once('@').map(|(_, host)| host).unwrap_or(auth);
    if let Some((name, port)) = host.rsplit_once(':') {
        if !name.is_empty() && !port.is_empty() && port.bytes().all(|byte| byte.is_ascii_digit()) {
            return name.to_string();
        }
    }
    host.to_string()
}

/// True for an empty host, loopback, or this machine's hostname (and `.local` / short aliases).
pub fn is_local_host(host: &str) -> bool {
    let host = host.trim().trim_matches(['[', ']']);
    if host.is_empty() {
        return true;
    }
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    if matches!(host.as_str(), "localhost" | "127.0.0.1" | "::1" | "0.0.0.0") {
        return true;
    }
    system_hostname().is_some_and(|local| host_names_match(&host, &local))
}

fn host_names_match(left: &str, right: &str) -> bool {
    let left = host_aliases(left);
    let right = host_aliases(right);
    left.iter().any(|alias| right.iter().any(|other| alias == other))
}

fn host_aliases(name: &str) -> Vec<String> {
    let name = name.trim_end_matches('.').to_ascii_lowercase();
    let mut aliases = vec![name.clone()];
    if let Some(short) = name.strip_suffix(".local") {
        aliases.push(short.to_string());
    } else if !name.contains('.') {
        aliases.push(format!("{name}.local"));
    }
    if let Some((short, _)) = name.split_once('.') {
        if !short.is_empty() {
            aliases.push(short.to_string());
        }
    }
    aliases
}

fn system_hostname() -> Option<String> {
    #[cfg(unix)]
    {
        unix_hostname()
    }
    #[cfg(windows)]
    {
        windows_hostname()
    }
    #[cfg(not(any(unix, windows)))]
    {
        None
    }
}

#[cfg(unix)]
fn unix_hostname() -> Option<String> {
    let mut buf = [0u8; 256];
    unsafe extern "C" {
        fn gethostname(name: *mut core::ffi::c_char, len: usize) -> i32;
    }
    let rc = unsafe { gethostname(buf.as_mut_ptr().cast(), buf.len()) };
    if rc != 0 {
        return None;
    }
    let end = buf.iter().position(|&byte| byte == 0).unwrap_or(buf.len());
    let name = std::str::from_utf8(&buf[..end]).ok()?.trim();
    if name.is_empty() { None } else { Some(name.to_string()) }
}

#[cfg(windows)]
fn windows_hostname() -> Option<String> {
    let mut buf = [0u16; 256];
    let mut len = buf.len() as u32;
    unsafe extern "system" {
        fn GetComputerNameW(buffer: *mut u16, size: *mut u32) -> i32;
    }
    let ok = unsafe { GetComputerNameW(buf.as_mut_ptr(), &raw mut len) };
    if ok == 0 || len == 0 {
        return None;
    }
    let name = String::from_utf16(buf.get(..len as usize)?).ok()?;
    let name = name.trim();
    if name.is_empty() { None } else { Some(name.to_string()) }
}

pub fn usable_cwd(path: Option<&Path>) -> Option<PathBuf> {
    path.filter(|path| path.is_dir()).map(Path::to_path_buf)
}

pub fn usable_local_dir(cwd: Option<&TerminalCwd>) -> Option<PathBuf> {
    cwd.filter(|cwd| !cwd.is_remote())
        .and_then(|cwd| usable_cwd(Some(cwd.path.as_path())))
}

pub fn process_cwd(pid: u32) -> Option<PathBuf> {
    #[cfg(target_os = "linux")]
    {
        std::fs::read_link(format!("/proc/{pid}/cwd")).ok()
    }
    #[cfg(target_os = "macos")]
    {
        macos_process_cwd(pid)
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = pid;
        None
    }
}

fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            if let (Some(high), Some(low)) = (from_hex(bytes[index + 1]), from_hex(bytes[index + 2])) {
                out.push((high << 4) | low);
                index += 3;
                continue;
            }
        }
        out.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn from_hex(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn normalize_file_path(path: String) -> String {
    #[cfg(windows)]
    {
        let bytes = path.as_bytes();
        if bytes.len() >= 3 && bytes[0] == b'/' && bytes[2] == b':' {
            return path[1..].to_string();
        }
    }
    path
}

#[cfg(target_os = "macos")]
fn macos_process_cwd(pid: u32) -> Option<PathBuf> {
    const PROC_PIDVNODEPATHINFO: i32 = 9;
    const MAXPATHLEN: usize = 1024;

    #[repr(C)]
    struct VnodeInfoPath {
        _opaque: [u8; 152],
        vip_path: [u8; MAXPATHLEN],
    }

    #[repr(C)]
    struct ProcVnodePathInfo {
        pvi_cdir: VnodeInfoPath,
        pvi_rdir: VnodeInfoPath,
    }

    unsafe extern "C" {
        fn proc_pidinfo(pid: i32, flavor: i32, arg: u64, buffer: *mut core::ffi::c_void, buffersize: i32) -> i32;
    }

    let mut info = unsafe { std::mem::zeroed::<ProcVnodePathInfo>() };
    let size = std::mem::size_of::<ProcVnodePathInfo>() as i32;
    let wrote = unsafe { proc_pidinfo(pid as i32, PROC_PIDVNODEPATHINFO, 0, (&raw mut info).cast(), size) };
    if wrote < size {
        return None;
    }
    let end = info
        .pvi_cdir
        .vip_path
        .iter()
        .position(|&byte| byte == 0)
        .unwrap_or(MAXPATHLEN);
    let path = std::str::from_utf8(&info.pvi_cdir.vip_path[..end]).ok()?;
    if path.is_empty() {
        return None;
    }
    Some(PathBuf::from(path))
}

#[cfg(test)]
mod tests {
    use super::{TerminalCwd, is_local_host, parse_pwd, usable_local_dir};
    use std::path::PathBuf;

    #[test]
    fn parse_pwd_reads_plain_and_local_file_urls() {
        assert_eq!(parse_pwd("/tmp/project"), Some(TerminalCwd::local("/tmp/project")));
        assert_eq!(parse_pwd("file:///tmp/project"), Some(TerminalCwd::local("/tmp/project")));
        assert_eq!(parse_pwd("file:/tmp/project"), Some(TerminalCwd::local("/tmp/project")));
        assert_eq!(parse_pwd("file://localhost/Users/dev/src"), Some(TerminalCwd::local("/Users/dev/src")));
        assert_eq!(parse_pwd("file://127.0.0.1/tmp/project"), Some(TerminalCwd::local("/tmp/project")));
        assert_eq!(parse_pwd("file://[::1]/tmp/project"), Some(TerminalCwd::local("/tmp/project")));
        assert_eq!(parse_pwd("file://user@localhost/tmp/project"), Some(TerminalCwd::local("/tmp/project")));
        assert_eq!(parse_pwd("file:///tmp/my%20project"), Some(TerminalCwd::local("/tmp/my project")));
        assert_eq!(parse_pwd(""), None);
        assert_eq!(parse_pwd("   "), None);
    }

    #[test]
    fn parse_pwd_keeps_remote_hosts() {
        let cwd = parse_pwd("file://otherbox/home/you/src").unwrap();
        assert!(cwd.is_remote());
        assert_eq!(cwd.host.as_deref(), Some("otherbox"));
        assert_eq!(cwd.path, PathBuf::from("/home/you/src"));
        let cwd = parse_pwd("file://user@otherbox/var/log").unwrap();
        assert_eq!(cwd.host.as_deref(), Some("otherbox"));
        assert_eq!(cwd.path, PathBuf::from("/var/log"));
        let cwd = parse_pwd("file://otherbox:22/tmp").unwrap();
        assert_eq!(cwd.host.as_deref(), Some("otherbox"));
    }

    #[test]
    fn remote_paths_are_not_usable_local_dirs() {
        let tmp = std::env::temp_dir();
        assert!(tmp.is_dir());
        let remote = TerminalCwd::remote("otherbox", tmp.clone());
        assert!(usable_local_dir(Some(&remote)).is_none());
        assert!(usable_local_dir(Some(&TerminalCwd::local(tmp))).is_some());
    }

    #[test]
    fn localhost_aliases_are_local() {
        assert!(is_local_host(""));
        assert!(is_local_host("localhost"));
        assert!(is_local_host("LOCALHOST"));
        assert!(is_local_host("127.0.0.1"));
        assert!(is_local_host("::1"));
        assert!(!is_local_host("otherbox"));
        assert!(!is_local_host("otherbox.example"));
    }
}
