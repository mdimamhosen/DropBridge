//! Bonjour/mDNS discovery. Interoperates with Android `NsdManager` and macOS `NWBrowser`.
use anyhow::Result;
use mdns_sd::{ServiceDaemon, ServiceEvent, ServiceInfo};
use std::collections::HashMap;

pub const SERVICE: &str = "_dropbridge._tcp.local.";

pub enum Found {
    Up { id: String, name: String, kind: String, addrs: Vec<String>, port: u16 },
    Down { id: String },
}

/// Advertise this device and browse for others. Keep the returned daemon alive.
pub fn start(id: &str, name: &str, kind: &str, port: u16, cb: impl Fn(Found) + Send + 'static) -> Result<ServiceDaemon> {
    let d = ServiceDaemon::new()?;
    let inst = &id[..12];
    let props = [("id", id), ("n", name), ("k", kind), ("v", "1")];
    let info = ServiceInfo::new(SERVICE, inst, &format!("db-{inst}.local."), "", port, &props[..])?.enable_addr_auto();
    d.register(info)?;
    let rx = d.browse(SERVICE)?;
    let own = id.to_string();
    std::thread::spawn(move || {
        let mut names: HashMap<String, String> = HashMap::new();
        let mut seen: HashMap<String, (Vec<std::net::IpAddr>, u16)> = HashMap::new();
        while let Ok(ev) = rx.recv() {
            match ev {
                ServiceEvent::ServiceResolved(info) => {
                    let Some(pid) = info.get_property_val_str("id").map(str::to_string) else {
                        continue;
                    };
                    if pid == own {
                        continue;
                    }
                    // Link-local IPv6 needs a scope id we can't carry in a "host" string; skip it.
                    let mut addrs: Vec<_> = info
                        .get_addresses()
                        .iter()
                        .filter(|a| !a.is_loopback() && !matches!(a, std::net::IpAddr::V6(v6) if (v6.segments()[0] & 0xffc0) == 0xfe80))
                        .copied()
                        .collect();
                    addrs.sort_by_key(|a| (a.is_ipv6(), *a)); // IPv4 first
                    if addrs.is_empty() || seen.get(&pid) == Some(&(addrs.clone(), info.get_port())) {
                        continue; // resolves arrive once per address record; report each change once
                    }
                    seen.insert(pid.clone(), (addrs.clone(), info.get_port()));
                    names.insert(info.get_fullname().to_string(), pid.clone());
                    cb(Found::Up {
                        id: pid,
                        name: info.get_property_val_str("n").unwrap_or("Unknown device").to_string(),
                        kind: info.get_property_val_str("k").unwrap_or("").to_string(),
                        addrs: addrs.iter().map(|a| a.to_string()).collect(),
                        port: info.get_port(),
                    });
                }
                ServiceEvent::ServiceRemoved(_, full) => {
                    if let Some(id) = names.remove(&full) {
                        seen.remove(&id);
                        cb(Found::Down { id });
                    }
                }
                _ => {}
            }
        }
    });
    Ok(d)
}
