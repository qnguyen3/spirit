use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteEndpoint {
    pub host: String,
    pub port: u16,
}

impl RemoteEndpoint {
    pub fn new(host: impl Into<String>, port: u16) -> Self {
        Self {
            host: host.into(),
            port,
        }
    }

    pub fn authority(&self) -> String {
        if self.host.contains(':') && !self.host.starts_with('[') {
            format!("[{}]:{}", self.host, self.port)
        } else {
            format!("{}:{}", self.host, self.port)
        }
    }

    pub fn url(&self) -> String {
        format!("http://{}", self.authority())
    }

    pub fn pairing_url(&self, token: &str) -> String {
        format!("{}/pair?t={token}", self.url())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AllowedHosts {
    pub port: u16,
    pub extra: Vec<String>,
}

impl AllowedHosts {
    pub fn loopback(port: u16) -> Self {
        Self {
            port,
            extra: Vec::new(),
        }
    }

    pub fn with_extra(port: u16, extra: Vec<String>) -> Self {
        Self { port, extra }
    }

    fn base_names(&self) -> [&'static str; 3] {
        ["127.0.0.1", "localhost", "[::1]"]
    }

    pub fn allows(&self, host_header: &str) -> bool {
        let Some((host, port)) = split_authority(host_header) else {
            return false;
        };
        match port {
            Some(port) if port == self.port => {}
            None if self.port == 80 => {}
            Some(_) | None => return false,
        }
        self.base_names().iter().any(|name| *name == host)
            || self.extra.iter().any(|extra| normalize_host(extra) == host)
    }

    pub fn origin_matches(&self, origin: &str, host_header: &str) -> bool {
        let origin = origin.trim().to_ascii_lowercase();
        let Some(origin_authority) = origin
            .strip_prefix("http://")
            .or_else(|| origin.strip_prefix("https://"))
        else {
            return false;
        };
        if origin_authority.contains('/') {
            return false;
        }
        let Some((origin_host, origin_port)) = split_authority(origin_authority) else {
            return false;
        };
        let Some((header_host, header_port)) = split_authority(host_header) else {
            return false;
        };
        self.allows(host_header) && origin_host == header_host && origin_port == header_port
    }
}

fn normalize_host(value: &str) -> String {
    let trimmed = value.trim().trim_end_matches('.').to_ascii_lowercase();
    if trimmed.contains(':') && !trimmed.starts_with('[') {
        format!("[{trimmed}]")
    } else {
        trimmed
    }
}

fn split_authority(authority: &str) -> Option<(String, Option<u16>)> {
    let authority = authority.trim();
    if authority.is_empty() {
        return None;
    }
    if let Some(rest) = authority.strip_prefix('[') {
        let (address, tail) = rest.split_once(']')?;
        let host = format!("[{}]", address.trim_end_matches('.').to_ascii_lowercase());
        return match tail {
            "" => Some((host, None)),
            tail => {
                let port = tail.strip_prefix(':')?.parse().ok()?;
                Some((host, Some(port)))
            }
        };
    }
    match authority.rsplit_once(':') {
        Some((host, port)) if !host.is_empty() && !port.is_empty() => {
            let port = port.parse().ok()?;
            Some((normalize_host(host), Some(port)))
        }
        Some(_) => None,
        None => Some((normalize_host(authority), None)),
    }
}

#[cfg(test)]
#[path = "hosts_tests.rs"]
mod tests;
