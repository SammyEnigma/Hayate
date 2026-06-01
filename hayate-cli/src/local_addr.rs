use std::net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket};

pub fn local_ipv4s() -> Vec<Ipv4Addr> {
    let mut ips = local_ip_address_ipv4s();

    for ip in interface_ipv4s() {
        if !ips.contains(&ip) {
            ips.push(ip);
        }
    }

    for ip in route_probe_ipv4s() {
        if !ips.contains(&ip) {
            ips.push(ip);
        }
    }

    ips
}

pub fn local_subnets() -> Vec<Ipv4Addr> {
    let mut bases = Vec::new();
    for ip in local_ipv4s() {
        let o = ip.octets();
        let base = Ipv4Addr::new(o[0], o[1], o[2], 0);
        if !bases.contains(&base) {
            bases.push(base);
        }
    }
    bases
}

pub fn primary_local_ipv4() -> Option<Ipv4Addr> {
    local_ipv4s().into_iter().next()
}

pub fn is_local_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ipv4) => local_ipv4s().contains(&ipv4),
        IpAddr::V6(_) => {
            if let Ok(ifaces) = get_if_addrs::get_if_addrs() {
                return ifaces.into_iter().any(|iface| iface.ip() == ip);
            }
            false
        }
    }
}

fn interface_ipv4s() -> Vec<Ipv4Addr> {
    let mut ips = Vec::new();
    if let Ok(ifaces) = get_if_addrs::get_if_addrs() {
        for iface in ifaces {
            if iface.is_loopback() {
                continue;
            }
            if let get_if_addrs::IfAddr::V4(ifv4) = iface.addr
                && is_usable_local_ipv4(ifv4.ip)
                && !ips.contains(&ifv4.ip)
            {
                ips.push(ifv4.ip);
            }
        }
    }
    ips
}

fn local_ip_address_ipv4s() -> Vec<Ipv4Addr> {
    let mut ips = Vec::new();

    if let Ok(IpAddr::V4(ip)) = local_ip_address::local_ip()
        && is_usable_local_ipv4(ip)
    {
        ips.push(ip);
    }

    if let Ok(ifaces) = local_ip_address::list_afinet_netifas() {
        for (_name, ip) in ifaces {
            let IpAddr::V4(ip) = ip else {
                continue;
            };
            if is_usable_local_ipv4(ip) && !ips.contains(&ip) {
                ips.push(ip);
            }
        }
    }

    ips
}

fn route_probe_ipv4s() -> Vec<Ipv4Addr> {
    let mut ips = Vec::new();
    let probes = [
        SocketAddr::from(([8, 8, 8, 8], 80)),
        SocketAddr::from(([1, 1, 1, 1], 80)),
        SocketAddr::from(([192, 168, 1, 1], 80)),
        SocketAddr::from(([10, 0, 0, 1], 80)),
    ];

    for target in probes {
        let Ok(socket) = UdpSocket::bind("0.0.0.0:0") else {
            continue;
        };
        if socket.connect(target).is_err() {
            continue;
        }
        let Ok(local_addr) = socket.local_addr() else {
            continue;
        };
        let IpAddr::V4(ip) = local_addr.ip() else {
            continue;
        };
        if is_usable_local_ipv4(ip) && !ips.contains(&ip) {
            ips.push(ip);
        }
    }

    ips
}

fn is_usable_local_ipv4(ip: Ipv4Addr) -> bool {
    !ip.is_loopback()
        && !ip.is_unspecified()
        && !ip.is_multicast()
        && !ip.is_broadcast()
        && !ip.is_documentation()
}
