//! Pin HDMI output bindings to the display, not the ALSA sink name. A GPU's
//! HDMI sinks are named by PCM slot (`hdmi-stereo-extraN`), and the slot a
//! display lands on moves with cabling, so a binding saved by sink name can
//! end up playing into a different monitor.

use crate::routing_model::RoutingModel;
use serde_json::Value;
use std::process::Command;

#[derive(Debug, Clone, PartialEq)]
pub struct HdmiPort {
    pub card: String,
    pub profile: String,
    pub sink: String,
    pub display: String,
}

/// Connected HDMI ports from `pactl -f json list cards` output.
pub fn parse(cards: &str) -> Vec<HdmiPort> {
    let Ok(Value::Array(cards)) = serde_json::from_str::<Value>(cards) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for card in &cards {
        let Some(name) = card["name"].as_str() else {
            continue;
        };
        let Some(suffix) = name.strip_prefix("alsa_card.") else {
            continue;
        };
        let Some(ports) = card["ports"].as_object() else {
            continue;
        };
        for port in ports.values() {
            let props = &port["properties"];
            if props["port.type"].as_str() != Some("hdmi")
                || port["availability"].as_str() == Some("not available")
            {
                continue;
            }
            let Some(display) = props["device.product.name"].as_str() else {
                continue;
            };
            // Stereo profile only: it is what WaveSink's stereo mixes link to.
            let Some(profile) = port["profiles"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .find(|p| p.starts_with("output:hdmi-stereo"))
            else {
                continue;
            };
            out.push(HdmiPort {
                card: name.to_string(),
                profile: profile.to_string(),
                sink: format!("alsa_output.{suffix}.{}", &profile["output:".len()..]),
                display: display.to_string(),
            });
        }
    }
    out
}

fn ports() -> Vec<HdmiPort> {
    Command::new("pactl")
        .args(["-f", "json", "list", "cards"])
        .output()
        .ok()
        .map(|o| parse(&String::from_utf8_lossy(&o.stdout)))
        .unwrap_or_default()
}

/// Display currently behind an HDMI sink.
pub fn display_of(ports: &[HdmiPort], sink: &str) -> Option<String> {
    ports
        .iter()
        .find(|p| p.sink == sink)
        .map(|p| p.display.clone())
}

/// Record the display behind every HDMI binding that lacks one.
pub fn stamp(model: &mut RoutingModel) -> bool {
    stamp_with(&ports(), model)
}

fn stamp_with(ports: &[HdmiPort], model: &mut RoutingModel) -> bool {
    let mut changed = false;
    for b in model
        .mixes
        .iter_mut()
        .flat_map(|m| m.output_bindings.iter_mut())
    {
        if b.display.is_none() {
            b.display = display_of(ports, &b.device);
            changed |= b.display.is_some();
        }
    }
    changed
}

/// Card profile switches needed so each pinned display's sink exists, with
/// bindings rewritten to the sink the display is on now. Pure for tests.
// ponytail: two identical monitors (same EDID name) are indistinguishable;
// the current sink wins, else the first match. Add EDID serial when needed.
pub fn plan(ports: &[HdmiPort], model: &mut RoutingModel) -> Vec<(String, String)> {
    let mut switches = Vec::new();
    for b in model
        .mixes
        .iter_mut()
        .flat_map(|m| m.output_bindings.iter_mut())
    {
        let Some(display) = b.display.as_deref() else {
            continue;
        };
        let here = |p: &&HdmiPort| p.display == display;
        if ports.iter().filter(here).any(|p| p.sink == b.device) {
            continue;
        }
        let Some(port) = ports.iter().find(here) else {
            continue;
        };
        b.device = port.sink.clone();
        let switch = (port.card.clone(), port.profile.clone());
        if !switches.contains(&switch) {
            switches.push(switch);
        }
    }
    switches
}

/// Stamp, re-point moved displays and switch card profiles. True when the
/// model changed and should be saved.
// ponytail: runs at startup and profile load only; a display re-cabled while
// WaveSink runs is picked up on the next restart. Hook card events if needed.
pub fn repin(model: &mut RoutingModel) -> bool {
    let ports = ports();
    let before = model.clone();
    stamp_with(&ports, model);
    let switches = plan(&ports, model);
    for (card, profile) in &switches {
        eprintln!("wavesink: display moved, switching {card} to {profile}");
        let ok = Command::new("pactl")
            .args(["set-card-profile", card, profile])
            .status()
            .is_ok_and(|s| s.success());
        if !ok {
            eprintln!("wavesink: set-card-profile {card} {profile} failed");
        }
    }
    *model != before
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::routing_model::OutputBinding;

    const CARDS: &str = r#"[{"name":"alsa_card.pci-0000_01_00.1","ports":{
      "hdmi-output-0":{"availability":"available","properties":{"port.type":"hdmi","device.product.name":"Odyssey G5"},"profiles":["output:hdmi-stereo"]},
      "hdmi-output-1":{"availability":"not available","properties":{"port.type":"hdmi"},"profiles":["output:hdmi-stereo-extra1"]},
      "hdmi-output-3":{"availability":"available","properties":{"port.type":"hdmi","device.product.name":"Elgato1440HDR"},"profiles":["output:hdmi-stereo-extra3"]}}}]"#;

    #[test]
    fn follows_display_to_new_slot() {
        let ports = parse(CARDS);
        assert_eq!(ports.len(), 2);
        assert_eq!(
            display_of(&ports, "alsa_output.pci-0000_01_00.1.hdmi-stereo-extra3").as_deref(),
            Some("Elgato1440HDR")
        );

        let mut model: RoutingModel = serde_json::from_str(
            r#"{"version":99,"inputs":[],"mixes":[{"id":"sink_bus_stream","label":"Stream","order":0,
                "output_bindings":[{"device":"alsa_output.pci-0000_01_00.1.hdmi-stereo-extra2","display":"Elgato1440HDR"},
                                   {"device":"alsa_output.usb.analog-stereo"}]}]}"#,
        ).expect("model");
        let switches = plan(&ports, &mut model);
        assert_eq!(
            switches,
            vec![(
                "alsa_card.pci-0000_01_00.1".into(),
                "output:hdmi-stereo-extra3".into()
            )]
        );
        assert_eq!(
            model.mixes[0].output_bindings[0].device,
            "alsa_output.pci-0000_01_00.1.hdmi-stereo-extra3"
        );
        assert_eq!(
            model.mixes[0].output_bindings[1],
            OutputBinding {
                device: "alsa_output.usb.analog-stereo".into(),
                enabled: true,
                display: None
            }
        );
        assert!(
            plan(&ports, &mut model).is_empty(),
            "already pinned: no switch"
        );
    }
}
