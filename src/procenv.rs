//! Reading other processes' environment and command lines from /proc.

/// The value of `name` in the environment of `pid`.
pub fn env_var(pid: u32, name: &str) -> Option<String> {
    let raw = std::fs::read(format!("/proc/{pid}/environ")).ok()?;
    parse_environ(&raw, name)
}

pub fn parse_environ(raw: &[u8], name: &str) -> Option<String> {
    let prefix = format!("{name}=");
    raw.split(|b| *b == 0)
        .find_map(|kv| kv.strip_prefix(prefix.as_bytes()))
        .map(|v| String::from_utf8_lossy(v).into_owned())
}

/// Every process (other than us) whose environment has `name=value`.
pub fn with_env(name: &str, value: &str) -> Vec<u32> {
    let me = std::process::id();
    std::fs::read_dir("/proc")
        .map(|rd| {
            rd.flatten()
                .filter_map(|e| e.file_name().to_string_lossy().parse::<u32>().ok())
                .filter(|pid| *pid != me && env_var(*pid, name).as_deref() == Some(value))
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn environ_parsing() {
        let raw = b"PATH=/bin\0SteamGameId=123\0EMPTY=\0";
        assert_eq!(parse_environ(raw, "SteamGameId").as_deref(), Some("123"));
        assert_eq!(parse_environ(raw, "EMPTY").as_deref(), Some(""));
        assert_eq!(parse_environ(raw, "Steam"), None);
        // Our own environment is readable.
        std::env::var("PATH").ok().map(|p| assert_eq!(env_var(std::process::id(), "PATH"), Some(p)));
    }
}
