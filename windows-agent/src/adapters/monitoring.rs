use std::collections::{HashMap, HashSet};

use super::{discover_adapters, InterfaceKind, NetworkAdapter, OperStatus};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InterfaceEvent {
    Added,
    Removed,
    Changed,
    Recovered,
    Down,
}

#[derive(Debug, Clone, PartialEq)]
pub struct InterfaceHealth {
    pub adapter: NetworkAdapter,
    pub latency_ms: u32,
    pub packet_loss_pct: f64,
    pub link_score: u8,
    pub event: Option<InterfaceEvent>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InterfaceChange {
    pub if_index: u32,
    pub event: InterfaceEvent,
}

#[derive(Debug, Default)]
pub struct InterfaceMonitor {
    previous: HashMap<u32, NetworkAdapter>,
}

impl InterfaceMonitor {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn poll(&mut self) -> Result<Vec<InterfaceHealth>, Box<dyn std::error::Error>> {
        let current = discover_adapters()?;
        let current_map: HashMap<u32, NetworkAdapter> =
            current.iter().map(|adapter| (adapter.if_index, adapter.clone())).collect();

        let mut result = Vec::with_capacity(current.len());
        let mut seen = HashSet::new();

        for adapter in current {
            let if_index = adapter.if_index;
            seen.insert(if_index);

            let previous = self.previous.get(&if_index).cloned();
            let event = evaluate_event(previous.as_ref(), &adapter);

            let health = InterfaceHealth {
                latency_ms: estimate_latency_ms(&adapter),
                packet_loss_pct: estimate_packet_loss(&adapter),
                link_score: score_interface(&adapter),
                event,
                adapter: adapter.clone(),
            };

            result.push(health);
        }

        for (if_index, old_adapter) in &self.previous {
            if !seen.contains(if_index) {
                result.push(InterfaceHealth {
                    latency_ms: 0,
                    packet_loss_pct: 100.0,
                    link_score: 0,
                    event: Some(InterfaceEvent::Removed),
                    adapter: old_adapter.clone(),
                });
            }
        }

        self.previous = current_map;
        Ok(result)
    }

    pub fn diff(&mut self) -> Result<Vec<InterfaceChange>, Box<dyn std::error::Error>> {
        let current = discover_adapters()?;
        let current_map: HashMap<u32, NetworkAdapter> =
            current.iter().map(|adapter| (adapter.if_index, adapter.clone())).collect();

        let mut changes = Vec::new();
        let mut seen = HashSet::new();

        for adapter in &current {
            let if_index = adapter.if_index;
            seen.insert(if_index);
            if let Some(previous) = self.previous.get(&if_index) {
                if previous.oper_status != adapter.oper_status
                    || previous.kind != adapter.kind
                    || previous.ipv4 != adapter.ipv4
                    || previous.ipv6 != adapter.ipv6
                {
                    let event = if adapter.is_active && !previous.is_active {
                        InterfaceEvent::Recovered
                    } else {
                        InterfaceEvent::Changed
                    };
                    changes.push(InterfaceChange {
                        if_index,
                        event,
                    });
                }
            } else {
                changes.push(InterfaceChange {
                    if_index,
                    event: InterfaceEvent::Added,
                });
            }
        }

        for (if_index, old_adapter) in &self.previous {
            if !seen.contains(if_index) {
                changes.push(InterfaceChange {
                    if_index: *if_index,
                    event: if old_adapter.is_active {
                        InterfaceEvent::Removed
                    } else {
                        InterfaceEvent::Down
                    },
                });
            }
        }

        self.previous = current_map;
        Ok(changes)
    }
}

fn evaluate_event(previous: Option<&NetworkAdapter>, adapter: &NetworkAdapter) -> Option<InterfaceEvent> {
    let previous = match previous {
        Some(previous) => previous,
        None => {
            return Some(if adapter.is_active {
                InterfaceEvent::Added
            } else {
                InterfaceEvent::Down
            });
        }
    };

    if !adapter.is_active && previous.is_active {
        return Some(InterfaceEvent::Down);
    }

    if adapter.is_active && !previous.is_active {
        return Some(InterfaceEvent::Recovered);
    }

    if previous.oper_status != adapter.oper_status
        || previous.kind != adapter.kind
        || previous.ipv4 != adapter.ipv4
        || previous.ipv6 != adapter.ipv6
    {
        Some(InterfaceEvent::Changed)
    } else {
        None
    }
}

fn score_interface(adapter: &NetworkAdapter) -> u8 {
    let mut score = 100u8;

    if !adapter.is_active {
        return 0;
    }

    match adapter.oper_status {
        OperStatus::Down | OperStatus::NotPresent | OperStatus::LowerLayerDown => return 0,
        OperStatus::Dormant => score = 35,
        OperStatus::Testing => score = 60,
        _ => {}
    }

    if matches!(adapter.kind, InterfaceKind::Loopback) {
        return 100;
    }

    if adapter.ipv4.is_empty() && adapter.ipv6.is_empty() {
        score = score.saturating_sub(30);
    }

    if matches!(adapter.kind, InterfaceKind::Wifi) {
        score = score.saturating_sub(10);
    }

    if matches!(adapter.kind, InterfaceKind::Ethernet) {
        score = score.saturating_sub(5);
    }

    score
}

fn estimate_latency_ms(adapter: &NetworkAdapter) -> u32 {
    if !adapter.is_active {
        return 0;
    }

    let base = match adapter.kind {
        InterfaceKind::Wifi => 18,
        InterfaceKind::Ethernet => 5,
        InterfaceKind::Loopback => 1,
        InterfaceKind::Tunnel => 12,
        InterfaceKind::Other => 20,
    };

    let penalty = if adapter.ipv4.is_empty() && adapter.ipv6.is_empty() {
        40
    } else {
        0
    };

    base + penalty
}

fn estimate_packet_loss(adapter: &NetworkAdapter) -> f64 {
    if !adapter.is_active {
        return 100.0;
    }

    if adapter.ipv4.is_empty() && adapter.ipv6.is_empty() {
        return 12.5;
    }

    match adapter.oper_status {
        OperStatus::Dormant => 8.0,
        OperStatus::Testing => 4.0,
        _ => 0.5,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn score_interface_marks_down_inactive_link() {
        let adapter = NetworkAdapter {
            if_index: 1,
            name: "Wi-Fi".into(),
            friendly_name: "Wi-Fi".into(),
            description: "Intel Wi-Fi".into(),
            kind: InterfaceKind::Wifi,
            oper_status: OperStatus::Down,
            mtu: 1500,
            mac_address: None,
            ipv4: vec![],
            ipv6: vec![],
            is_active: false,
        };

        assert_eq!(score_interface(&adapter), 0);
    }

    #[test]
    fn estimate_latency_is_lower_for_ethernet() {
        let adapter = NetworkAdapter {
            if_index: 2,
            name: "Ethernet".into(),
            friendly_name: "Ethernet".into(),
            description: "Realtek Ethernet".into(),
            kind: InterfaceKind::Ethernet,
            oper_status: OperStatus::Up,
            mtu: 1500,
            mac_address: None,
            ipv4: vec!["192.168.0.10".parse().unwrap()],
            ipv6: vec![],
            is_active: true,
        };

        assert!(estimate_latency_ms(&adapter) < 20);
    }

    #[test]
    fn event_detection_handles_transition_to_recovered() {
        let previous = NetworkAdapter {
            if_index: 3,
            name: "USB".into(),
            friendly_name: "USB".into(),
            description: "USB tether".into(),
            kind: InterfaceKind::Other,
            oper_status: OperStatus::Down,
            mtu: 1500,
            mac_address: None,
            ipv4: vec![],
            ipv6: vec![],
            is_active: false,
        };

        let current = NetworkAdapter {
            if_index: 3,
            name: "USB".into(),
            friendly_name: "USB".into(),
            description: "USB tether".into(),
            kind: InterfaceKind::Other,
            oper_status: OperStatus::Up,
            mtu: 1500,
            mac_address: None,
            ipv4: vec!["10.42.0.25".parse().unwrap()],
            ipv6: vec![],
            is_active: true,
        };

        assert_eq!(evaluate_event(Some(&previous), &current), Some(InterfaceEvent::Recovered));
    }
}
