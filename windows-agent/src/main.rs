mod adapters;

use adapters::{discover_adapters, InterfaceKind};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let adapters = discover_adapters()?;

    if adapters.is_empty() {
        println!("No network adapters detected.");
        return Ok(());
    }

    for adapter in &adapters {
        println!(
            "[{}] {} | kind={:?} | status={:?} | mtu={} | active={} | mac={}",
            adapter.if_index,
            adapter.friendly_name,
            adapter.kind,
            adapter.oper_status,
            adapter.mtu,
            adapter.is_active,
            adapter.mac_address.as_deref().unwrap_or("n/a")
        );

        if !adapter.ipv4.is_empty() {
            println!("  ipv4: {:?}", adapter.ipv4);
        }
        if !adapter.ipv6.is_empty() {
            println!("  ipv6: {:?}", adapter.ipv6);
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adapter_primitives_compile() {
        let kind = InterfaceKind::Wifi;
        assert_eq!(format!("{kind}"), "wifi");
    }
}
