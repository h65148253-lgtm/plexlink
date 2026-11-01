#[cfg(target_os = "windows")]
mod implementation {
    use std::collections::BTreeSet;
    use std::net::{Ipv4Addr, Ipv6Addr};

    use windows::core::PWSTR;
    use windows::Win32::Foundation::{ERROR_BUFFER_OVERFLOW, ERROR_SUCCESS};
    use windows::Win32::NetworkManagement::IpHelper::{
        GetAdaptersAddresses, GAA_FLAG_INCLUDE_PREFIX, IP_ADAPTER_ADDRESSES_LH,
        IP_ADAPTER_UNICAST_ADDRESS_LH,
    };
    use windows::Win32::Networking::WinSock::{
        AF_INET, AF_INET6, SOCKADDR_IN, SOCKADDR_IN6, AF_UNSPEC,
    };

    use super::{InterfaceKind, NetworkAdapter, OperStatus};

    const IF_TYPE_ETHERNET_CSMACD: u32 = 6;
    const IF_TYPE_IEEE80211: u32 = 71;
    const IF_TYPE_IEEE80211_FH: u32 = 76;
    const IF_TYPE_IEEE80211_VIRTUAL: u32 = 77;
    const IF_TYPE_SOFTWARE_LOOPBACK: u32 = 24;
    const IF_TYPE_TUNNEL: u32 = 131;

    pub fn discover_adapters() -> Result<Vec<NetworkAdapter>, Box<dyn std::error::Error>> {
        let mut buffer_size = 0u32;

        let status = unsafe {
            GetAdaptersAddresses(
                AF_UNSPEC.0 as u32,
                GAA_FLAG_INCLUDE_PREFIX.0,
                None,
                None,
                &mut buffer_size,
            )
        };

        if status != ERROR_SUCCESS && status != ERROR_BUFFER_OVERFLOW {
            return Err(format!("GetAdaptersAddresses initial call failed: {status}").into());
        }

        let mut buffer = vec![0u8; buffer_size as usize];
        let mut adapters = Vec::new();
        let mut seen = BTreeSet::new();

        let status = unsafe {
            GetAdaptersAddresses(
                AF_UNSPEC.0 as u32,
                GAA_FLAG_INCLUDE_PREFIX.0,
                None,
                Some(buffer.as_mut_ptr() as *mut IP_ADAPTER_ADDRESSES_LH),
                &mut buffer_size,
            )
        };

        if status != ERROR_SUCCESS {
            return Err(format!("GetAdaptersAddresses final call failed: {status}").into());
        }

        let mut current = buffer.as_ptr() as *const IP_ADAPTER_ADDRESSES_LH;

        while !current.is_null() {
            let adapter = unsafe { &*current };
            let name = pwstr_to_string(adapter.AdapterName);
            if name.is_empty() {
                current = adapter.Next;
                continue;
            }

            if seen.insert(name.clone()) {
                let friendly_name = pwstr_to_string(adapter.FriendlyName);
                let description = pwstr_to_string(adapter.Description);
                let kind = match adapter.IfType {
                    IF_TYPE_ETHERNET_CSMACD => InterfaceKind::Ethernet,
                    IF_TYPE_IEEE80211 | IF_TYPE_IEEE80211_FH | IF_TYPE_IEEE80211_VIRTUAL => {
                        InterfaceKind::Wifi
                    }
                    IF_TYPE_SOFTWARE_LOOPBACK => InterfaceKind::Loopback,
                    IF_TYPE_TUNNEL => InterfaceKind::Tunnel,
                    _ => InterfaceKind::Other,
                };

                let oper_status = OperStatus::from_u32(adapter.OperStatus.0);
                let mtu = adapter.Mtu as u32;
                let mac_address = if adapter.PhysicalAddressLength > 0 {
                    let bytes = unsafe {
                        std::slice::from_raw_parts(
                            adapter.PhysicalAddress.as_ptr(),
                            adapter.PhysicalAddressLength as usize,
                        )
                    };
                    Some(format_mac(bytes))
                } else {
                    None
                };

                let ipv4 = collect_ipv4(adapter.FirstUnicastAddress);
                let ipv6 = collect_ipv6(adapter.FirstUnicastAddress);
                let is_active = matches!(oper_status, OperStatus::Up | OperStatus::Dormant)
                    && !matches!(kind, InterfaceKind::Loopback);

                adapters.push(NetworkAdapter {
                    if_index: adapter.IfIndex,
                    name: name.clone(),
                    friendly_name,
                    description,
                    kind,
                    oper_status,
                    mtu,
                    mac_address,
                    ipv4,
                    ipv6,
                    is_active,
                });
            }

            current = adapter.Next;
        }

        adapters.sort_by(|a, b| a.if_index.cmp(&b.if_index));
        Ok(adapters)
    }

    fn pwstr_to_string(value: PWSTR) -> String {
        if value.0.is_null() {
            return String::new();
        }

        let mut chars = Vec::new();
        let mut index = 0usize;

        unsafe {
            while *value.0.add(index) != 0 {
                chars.push(*value.0.add(index));
                index += 1;
            }
        }

        String::from_utf16_lossy(&chars)
    }

    fn format_mac(bytes: &[u8]) -> String {
        let mut out = String::new();
        for (index, byte) in bytes.iter().enumerate() {
            if index > 0 {
                out.push(':');
            }
            out.push_str(&format!("{:02x}", byte));
        }
        out
    }

    fn collect_ipv4(first: *mut IP_ADAPTER_UNICAST_ADDRESS_LH) -> Vec<Ipv4Addr> {
        let mut values = Vec::new();
        let mut current = first;

        unsafe {
            while !current.is_null() {
                let address = &*current;
                if let Some(sockaddr) = address.Address.lpSockaddr.as_ref() {
                    let sin = &*(sockaddr as *const SOCKADDR_IN);
                    if sin.sin_family == AF_INET as u16 {
                        let bytes = sin.sin_addr.S_un.S_addr.to_ne_bytes();
                        values.push(Ipv4Addr::new(bytes[0], bytes[1], bytes[2], bytes[3]));
                    }
                }
                current = address.Next;
            }
        }

        values
    }

    fn collect_ipv6(first: *mut IP_ADAPTER_UNICAST_ADDRESS_LH) -> Vec<Ipv6Addr> {
        let mut values = Vec::new();
        let mut current = first;

        unsafe {
            while !current.is_null() {
                let address = &*current;
                if let Some(sockaddr) = address.Address.lpSockaddr.as_ref() {
                    let sin6 = &*(sockaddr as *const SOCKADDR_IN6);
                    if sin6.sin6_family == AF_INET6 as u16 {
                        let bytes = sin6.sin6_addr.u.Byte;
                        values.push(Ipv6Addr::new(
                            ((bytes[0] as u16) << 8) | bytes[1] as u16,
                            ((bytes[2] as u16) << 8) | bytes[3] as u16,
                            ((bytes[4] as u16) << 8) | bytes[5] as u16,
                            ((bytes[6] as u16) << 8) | bytes[7] as u16,
                            ((bytes[8] as u16) << 8) | bytes[9] as u16,
                            ((bytes[10] as u16) << 8) | bytes[11] as u16,
                            ((bytes[12] as u16) << 8) | bytes[13] as u16,
                            ((bytes[14] as u16) << 8) | bytes[15] as u16,
                        ));
                    }
                }
                current = address.Next;
            }
        }

        values
    }
}

#[cfg(not(target_os = "windows"))]
mod implementation {
    use std::net::{Ipv4Addr, Ipv6Addr};

    use super::{InterfaceKind, NetworkAdapter, OperStatus};

    pub fn discover_adapters() -> Result<Vec<NetworkAdapter>, Box<dyn std::error::Error>> {
        Ok(vec![])
    }
}

pub use implementation::discover_adapters;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InterfaceKind {
    Ethernet,
    Wifi,
    Loopback,
    Tunnel,
    Other,
}

impl std::fmt::Display for InterfaceKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let label = match self {
            InterfaceKind::Ethernet => "ethernet",
            InterfaceKind::Wifi => "wifi",
            InterfaceKind::Loopback => "loopback",
            InterfaceKind::Tunnel => "tunnel",
            InterfaceKind::Other => "other",
        };
        write!(f, "{label}")
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OperStatus {
    Up,
    Down,
    Testing,
    Dormant,
    NotPresent,
    LowerLayerDown,
    Unknown,
}

impl OperStatus {
    pub fn from_u32(value: u32) -> Self {
        match value {
            1 => Self::Up,
            2 => Self::Down,
            3 => Self::Testing,
            4 => Self::Dormant,
            5 => Self::NotPresent,
            6 => Self::LowerLayerDown,
            _ => Self::Unknown,
        }
    }
}

#[derive(Debug, Clone)]
pub struct NetworkAdapter {
    pub if_index: u32,
    pub name: String,
    pub friendly_name: String,
    pub description: String,
    pub kind: InterfaceKind,
    pub oper_status: OperStatus,
    pub mtu: u32,
    pub mac_address: Option<String>,
    pub ipv4: Vec<Ipv4Addr>,
    pub ipv6: Vec<Ipv6Addr>,
    pub is_active: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interface_kind_formatting_is_consistent() {
        assert_eq!(InterfaceKind::Wifi.to_string(), "wifi");
        assert_eq!(InterfaceKind::Ethernet.to_string(), "ethernet");
    }

    #[test]
    fn oper_status_range_covers_known_values() {
        assert_eq!(OperStatus::from_u32(1), OperStatus::Up);
        assert_eq!(OperStatus::from_u32(2), OperStatus::Down);
        assert_eq!(OperStatus::from_u32(99), OperStatus::Unknown);
    }
}
