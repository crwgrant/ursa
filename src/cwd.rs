use std::path::{Path, PathBuf};

pub fn parse_pwd(raw: &str) -> Option<PathBuf> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    let path = if let Some(rest) = raw.strip_prefix("file://") {
        let decoded = percent_decode(rest);
        if decoded.starts_with('/') {
            decoded
        } else {
            let slash = decoded.find('/')?;
            decoded[slash..].to_string()
        }
    } else {
        raw.to_string()
    };
    let path = normalize_file_path(path);
    if path.is_empty() {
        return None;
    }
    Some(PathBuf::from(path))
}

pub fn usable_cwd(path: Option<&Path>) -> Option<PathBuf> {
    path.filter(|path| path.is_dir()).map(Path::to_path_buf)
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
    use super::parse_pwd;
    use std::path::PathBuf;

    #[test]
    fn parse_pwd_reads_plain_and_file_urls() {
        assert_eq!(parse_pwd("/tmp/project"), Some(PathBuf::from("/tmp/project")));
        assert_eq!(parse_pwd("file:///tmp/project"), Some(PathBuf::from("/tmp/project")));
        assert_eq!(parse_pwd("file://localhost/Users/dev/src"), Some(PathBuf::from("/Users/dev/src")));
        assert_eq!(parse_pwd("file:///tmp/my%20project"), Some(PathBuf::from("/tmp/my project")));
        assert_eq!(parse_pwd(""), None);
        assert_eq!(parse_pwd("   "), None);
    }
}
