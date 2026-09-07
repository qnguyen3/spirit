use std::net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket};

use remote_control::hosts::RemoteEndpoint;

pub(crate) fn lan_endpoints(port: u16) -> Vec<RemoteEndpoint> {
    let mut endpoints: Vec<RemoteEndpoint> = local_ipv4_addresses()
        .into_iter()
        .map(|address| RemoteEndpoint::new(address.to_string(), port))
        .collect();
    if let Some(name) = local_hostname() {
        endpoints.push(RemoteEndpoint::new(format!("{name}.local"), port));
    }
    endpoints
}

pub(crate) fn allowed_host_names(
    endpoints: &[RemoteEndpoint],
    allow_lan_access: bool,
) -> Vec<String> {
    if !allow_lan_access {
        return Vec::new();
    }
    let mut names: Vec<String> = endpoints
        .iter()
        .map(|endpoint| endpoint.host.clone())
        .collect();
    names.sort();
    names.dedup();
    names
}

fn local_ipv4_addresses() -> Vec<Ipv4Addr> {
    let mut addresses = Vec::new();
    if let Some(primary) = primary_ipv4_address() {
        addresses.push(primary);
    }
    for address in hostname_ipv4_addresses() {
        if !addresses.contains(&address) {
            addresses.push(address);
        }
    }
    addresses
}

fn primary_ipv4_address() -> Option<Ipv4Addr> {
    let socket = UdpSocket::bind(SocketAddr::from((Ipv4Addr::UNSPECIFIED, 0))).ok()?;
    socket
        .connect(SocketAddr::from((Ipv4Addr::new(203, 0, 113, 1), 80)))
        .ok()?;
    match socket.local_addr().ok()?.ip() {
        IpAddr::V4(address) if !address.is_loopback() && !address.is_unspecified() => Some(address),
        IpAddr::V4(_) | IpAddr::V6(_) => None,
    }
}

fn hostname_ipv4_addresses() -> Vec<Ipv4Addr> {
    use std::net::ToSocketAddrs as _;

    let Some(name) = local_hostname() else {
        return Vec::new();
    };
    let Ok(resolved) = (format!("{name}:0")).to_socket_addrs() else {
        return Vec::new();
    };
    resolved
        .filter_map(|address| match address.ip() {
            IpAddr::V4(address) if !address.is_loopback() && !address.is_unspecified() => {
                Some(address)
            }
            IpAddr::V4(_) | IpAddr::V6(_) => None,
        })
        .collect()
}

fn local_hostname() -> Option<String> {
    let raw = std::env::var("HOSTNAME")
        .ok()
        .or_else(|| std::fs::read_to_string("/etc/hostname").ok())
        .or_else(|| std::env::var("COMPUTERNAME").ok())?;
    let trimmed = raw.trim().trim_end_matches(".local");
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_ascii_lowercase())
    }
}

#[cfg(test)]
#[path = "lan_tests.rs"]
mod tests;
