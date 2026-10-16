//! Elgato vendor controls, independent of PipeWire routing. Wire facts:
//! OpenWave docs/protocol.md (MIT); see THIRD_PARTY_LICENSES.md.
use std::{fs, os::unix::fs::MetadataExt, path::Path, sync::Mutex, time::Duration};

use rusb::UsbContext;
use serde::{Deserialize, Serialize};

const TIMEOUT: Duration = Duration::from_millis(500);
// Serialize the complete read/modify/write/read-back, including UI polls.
static ACCESS: Mutex<()> = Mutex::new(());

#[derive(Clone, Copy)]
struct Profile {
    name: &'static str,
    pid: u16,
    length: usize,
    max_gain: f64,
    headphone_offset: usize,
    phantom_offset: Option<usize>,
    impedance_offset: Option<usize>,
    mix_offset: Option<usize>,
}

const XLR: Profile = Profile {
    name: "Wave XLR",
    pid: 0x007d,
    length: 34,
    max_gain: 80.0,
    headphone_offset: 9,
    phantom_offset: Some(6),
    impedance_offset: Some(33),
    mix_offset: None,
};
const PROFILES: [Profile; 4] = [
    XLR,
    Profile {
        name: "XLR Dock",
        pid: 0x00a6,
        ..XLR
    },
    Profile {
        name: "Wave:3",
        pid: 0x0070,
        length: 16,
        max_gain: 40.0,
        headphone_offset: 7,
        phantom_offset: None,
        impedance_offset: None,
        mix_offset: Some(10),
    },
    Profile {
        name: "XLR Dock MK.2",
        pid: 0x00c7,
        length: 38,
        ..XLR
    },
];

fn profile(vid: u16, pid: u16) -> Option<Profile> {
    PROFILES
        .iter()
        .copied()
        .find(|p| vid == 0x0fd9 && p.pid == pid)
}

#[derive(Debug, Serialize)]
pub struct Controls {
    pub token: String,
    pub model: &'static str,
    pub serial: String,
    pub gain_db: f64,
    pub max_gain_db: f64,
    pub muted: bool,
    pub headphone_db: f64,
    pub min_headphone_db: f64,
    pub phantom: Option<bool>,
    pub low_impedance: Option<bool>,
    /// 0 = microphone, 100 = PC. None when not mapped for this model.
    pub monitor_mix: Option<f64>,
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(
    tag = "control",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum Setting {
    Gain(f64),
    Mute(bool),
    Headphones(f64),
    Phantom(bool),
    LowImpedance(bool),
    MonitorMix(f64),
}

struct Target {
    profile: Profile,
    bus: u8,
    address: u8,
    serial: String,
    token: String,
}

fn io<T>(result: std::io::Result<T>) -> Result<T, String> {
    result.map_err(|e| format!("Elgato device identity: {e}"))
}

fn target_at(card_device: &Path) -> Result<Option<Target>, String> {
    let path = io(card_device.canonicalize())?;
    // Stop at the nearest USB device; never select an upstream hub.
    let Some(usb) = path.ancestors().find(|p| p.join("idVendor").exists()) else {
        return Ok(None);
    };
    let text = |name: &str| io(fs::read_to_string(usb.join(name))).map(|s| s.trim().to_owned());
    let hex = |name: &str| u16::from_str_radix(&text(name)?, 16).map_err(|e| e.to_string());
    let Some(profile) = profile(hex("idVendor")?, hex("idProduct")?) else {
        return Ok(None);
    };
    let bus = text("busnum")?.parse::<u8>().map_err(|e| e.to_string())?;
    let address = text("devnum")?.parse::<u8>().map_err(|e| e.to_string())?;
    let serial = match fs::read_to_string(usb.join("serial")) {
        Ok(s) => s.trim().to_owned(),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(e.to_string()),
    };
    // Sysfs directory identity survives control traffic and changes on replug.
    // usbfs /dev node ctime changes even on control reads; never use it here.
    let node = io(fs::metadata(usb))?;
    let token = format!(
        "{}:{bus}:{address}:{}:{}:{}:{serial}",
        profile.pid,
        node.ino(),
        node.ctime(),
        node.ctime_nsec()
    );
    Ok(Some(Target {
        profile,
        bus,
        address,
        serial,
        token,
    }))
}

fn find_target(card: u32) -> Result<Option<Target>, String> {
    target_at(Path::new(&format!("/sys/class/sound/card{card}/device")))
}

fn usb_error(e: rusb::Error) -> String {
    if e == rusb::Error::Access {
        "USB control permission denied. Install WaveSink's 70-wavesink-elgato.rules, reload udev rules, then reconnect this device. Audio routing needs no change.".into()
    } else {
        format!("Elgato USB controls: {e}. Close other vendor-control apps and refresh; writes are never retried automatically.")
    }
}

fn complete(actual: usize, expected: usize) -> Result<(), String> {
    if actual == expected {
        Ok(())
    } else {
        Err(format!(
            "Incomplete Elgato USB transfer: {actual}/{expected} bytes"
        ))
    }
}

fn read_block(
    handle: &rusb::DeviceHandle<rusb::Context>,
    mk2: bool,
    bank: u16,
    selector: u16,
    length: usize,
) -> Result<Vec<u8>, rusb::Error> {
    let mut bytes = vec![0; length];
    let mut read = || {
        handle.read_control(
            if mk2 { 0xc1 } else { 0xa1 },
            if mk2 { 1 } else { 0x85 },
            selector,
            bank,
            &mut bytes,
            TIMEOUT,
        )
    };
    let count = match read() {
        Err(rusb::Error::Io) if mk2 => read(),
        result => result,
    }?;
    bytes.truncate(count);
    Ok(bytes)
}

// Only short reads/stalls authorize trying the other documented MK.2 bank.
fn detect_bank(
    mut read: impl FnMut(u16, u16, usize) -> Result<Vec<u8>, rusb::Error>,
) -> Result<(u16, Config), String> {
    'bank: for bank in [0x0103, 0x0203] {
        let mut blocks = Vec::new();
        for (selector, length) in [(4, 38), (5, 2), (1, 6)] {
            match read(bank, selector, length) {
                Ok(bytes) if bytes.len() == length => blocks.push((selector, bytes)),
                Ok(_) | Err(rusb::Error::Pipe) => continue 'bank,
                Err(e) => return Err(usb_error(e)),
            }
        }
        return Ok((bank, Config(blocks)));
    }
    Err("Unknown XLR Dock MK.2 control bank; controls disabled".into())
}

struct Device {
    handle: rusb::DeviceHandle<rusb::Context>,
    bank: u16,
    target: Target,
}

impl Device {
    fn open(target: Target) -> Result<(Self, Config), String> {
        let context = rusb::Context::new().map_err(usb_error)?;
        let devices = context.devices().map_err(usb_error)?;
        let device = devices
            .iter()
            .find(|d| d.bus_number() == target.bus && d.address() == target.address)
            .ok_or("Elgato device disconnected")?;
        let descriptor = device.device_descriptor().map_err(usb_error)?;
        if descriptor.vendor_id() != 0x0fd9 || descriptor.product_id() != target.profile.pid {
            return Err("USB address changed; refresh hardware controls".into());
        }
        let handle = device.open().map_err(usb_error)?;
        if !target.serial.is_empty()
            && handle
                .read_serial_number_string_ascii(&descriptor)
                .map_err(usb_error)?
                != target.serial
        {
            return Err("USB serial changed; refresh hardware controls".into());
        }
        // Exclusive vendor-interface ownership, never detach snd-usb-audio,
        // reset a device, change configuration, or claim an audio interface.
        handle.claim_interface(3).map_err(usb_error)?;
        let (bank, config) = if target.profile.pid == 0x00c7 {
            detect_bank(|bank, selector, length| read_block(&handle, true, bank, selector, length))?
        } else {
            let bytes =
                read_block(&handle, false, 0x3303, 0, target.profile.length).map_err(usb_error)?;
            complete(bytes.len(), target.profile.length)?;
            (0x3303, Config(vec![(0, bytes)]))
        };
        Ok((
            Self {
                handle,
                bank,
                target,
            },
            config,
        ))
    }

    fn config(&self) -> Result<Config, String> {
        let mk2 = self.target.profile.pid == 0x00c7;
        let lengths = if mk2 {
            vec![(4, 38), (5, 2), (1, 6)]
        } else {
            vec![(0, self.target.profile.length)]
        };
        let mut blocks = Vec::new();
        for (selector, length) in lengths {
            let bytes =
                read_block(&self.handle, mk2, self.bank, selector, length).map_err(usb_error)?;
            complete(bytes.len(), length)?;
            blocks.push((selector, bytes));
        }
        Ok(Config(blocks))
    }
}

#[derive(Clone, Debug)]
struct Config(Vec<(u16, Vec<u8>)>);

impl Config {
    fn controls(&self, target: &Target) -> Controls {
        let p = target.profile;
        let b = &self.0[0].1;
        let mk2 = p.pid == 0x00c7;
        let u16_at = |offset| u16::from_le_bytes([b[offset], b[offset + 1]]);
        Controls {
            token: target.token.clone(),
            model: p.name,
            serial: target.serial.clone(),
            gain_db: if mk2 {
                f64::from(b[0])
            } else {
                f64::from(u16_at(0)) / 256.0
            },
            max_gain_db: p.max_gain,
            muted: if mk2 { b[1] & 1 != 0 } else { b[4] != 0 },
            headphone_db: if mk2 {
                -f64::from(self.0[1].1[0]) / 4.0
            } else {
                f64::from(u16_at(p.headphone_offset) as i16) / 256.0
            },
            min_headphone_db: if mk2 { -60.0 } else { -128.0 },
            phantom: if mk2 {
                Some(b[1] & 2 != 0)
            } else {
                p.phantom_offset.map(|o| b[o] != 0)
            },
            low_impedance: if mk2 {
                Some(self.0[1].1[1] & 2 != 0)
            } else {
                p.impedance_offset.map(|o| b[o] != 0)
            },
            monitor_mix: if mk2 {
                Some(f64::from(self.0[2].1[0]) / 2.0)
            } else {
                p.mix_offset.map(|o| f64::from(u16_at(o)) / 256.0)
            },
        }
    }

    fn apply(
        &mut self,
        p: Profile,
        setting: Setting,
        phantom_confirmed: bool,
    ) -> Result<(), String> {
        let mk2 = p.pid == 0x00c7;
        let bounded = |v: f64, min: f64, max: f64| {
            if v.is_finite() && (min..=max).contains(&v) {
                Ok(v)
            } else {
                Err(format!(
                    "Control value must be finite and within {min}..{max}"
                ))
            }
        };
        let put = |b: &mut [u8], o: usize, v: u16| b[o..o + 2].copy_from_slice(&v.to_le_bytes());
        let bit = |b: &mut u8, mask: u8, on: bool| *b = (*b & !mask) | if on { mask } else { 0 };
        match setting {
            Setting::Gain(db) => {
                let db = bounded(db, 0.0, p.max_gain)?;
                if mk2 {
                    self.0[0].1[0] = db.round() as u8;
                } else {
                    put(&mut self.0[0].1, 0, (db * 256.0).round() as u16);
                }
            }
            Setting::Mute(on) => {
                if mk2 {
                    bit(&mut self.0[0].1[1], 1, on);
                } else {
                    self.0[0].1[4] = u8::from(on);
                }
            }
            Setting::Phantom(on) => {
                if !phantom_confirmed {
                    return Err("Confirm microphone/cable compatibility and reduce monitoring before changing 48 V".into());
                }
                if mk2 {
                    bit(&mut self.0[0].1[1], 2, on);
                } else {
                    let o = p.phantom_offset.ok_or("This device has no phantom power")?;
                    self.0[0].1[o] = u8::from(on);
                }
            }
            Setting::Headphones(db) => {
                let db = bounded(db, if mk2 { -60.0 } else { -128.0 }, 0.0)?;
                if mk2 {
                    self.0[1].1[0] = (-db * 4.0).round() as u8;
                } else {
                    put(
                        &mut self.0[0].1,
                        p.headphone_offset,
                        ((db * 256.0).round() as i16) as u16,
                    );
                }
            }
            Setting::LowImpedance(on) => {
                if mk2 {
                    bit(&mut self.0[1].1[1], 2, on);
                } else {
                    let o = p
                        .impedance_offset
                        .ok_or("This device has no low impedance mode")?;
                    self.0[0].1[o] = u8::from(on);
                }
            }
            Setting::MonitorMix(percent) => {
                let percent = bounded(percent, 0.0, 100.0)?;
                if mk2 {
                    self.0[2].1[0] = (percent * 2.0).round() as u8;
                } else {
                    let o = p
                        .mix_offset
                        .ok_or("Monitor mix is not mapped for this device")?;
                    put(&mut self.0[0].1, o, (percent * 256.0).round() as u16);
                }
            }
        }
        Ok(())
    }
}

pub fn read(card: u32) -> Result<Option<Controls>, String> {
    let _guard = ACCESS.lock().map_err(|_| "USB control lock poisoned")?;
    let Some(target) = find_target(card)? else {
        return Ok(None);
    };
    let (device, config) = Device::open(target)?;
    Ok(Some(config.controls(&device.target)))
}

pub fn write(
    card: u32,
    token: &str,
    setting: Setting,
    phantom_confirmed: bool,
) -> Result<Controls, String> {
    let _guard = ACCESS.lock().map_err(|_| "USB control lock poisoned")?;
    let target = find_target(card)?.ok_or("Unsupported Elgato USB device")?;
    if target.token != token {
        return Err("Device connection changed; reopen hardware controls".into());
    }
    let (device, before) = Device::open(target)?;
    // Catch unplug/address reuse between sysfs discovery and opening the handle.
    // After this check, a disconnect invalidates this handle; it cannot retarget.
    if find_target(card)?.is_none_or(|current| current.token != device.target.token) {
        return Err("Device connection changed; reopen hardware controls".into());
    }
    let mut after = before.clone();
    after.apply(device.target.profile, setting, phantom_confirmed)?;
    // Only a changed, documented block is written. Never replay a failed write.
    for ((selector, bytes), (_, previous)) in after.0.iter().zip(&before.0) {
        if bytes == previous {
            continue;
        }
        let mk2 = device.target.profile.pid == 0x00c7;
        let count = device
            .handle
            .write_control(
                if mk2 { 0x41 } else { 0x21 },
                if mk2 { 1 } else { 5 },
                *selector,
                device.bank,
                bytes,
                TIMEOUT,
            )
            .map_err(usb_error)?;
        complete(count, bytes.len())?;
    }
    device
        .config()
        .map(|c| c.controls(&device.target))
        .map_err(|e| format!("Write may have succeeded; refresh actual hardware state. {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capabilities_and_reserved_bytes() {
        assert!(profile(0x0fd9, 0x00b6).is_none());
        assert!(profile(0x1234, 0x00a6).is_none());
        for p in PROFILES {
            let mk2 = p.pid == 0x00c7;
            let original = if mk2 {
                Config(vec![
                    (4, vec![0xa4; 38]),
                    (5, vec![0xa4; 2]),
                    (1, vec![0xa4; 6]),
                ])
            } else {
                Config(vec![(0, vec![0xa4; p.length])])
            };
            let mut c = original.clone();
            assert!(c.apply(p, Setting::Phantom(true), false).is_err());
            assert_eq!(c.0, original.0);
            if p.pid == 0x0070 {
                assert!(c.apply(p, Setting::Phantom(true), true).is_err());
                assert!(c.apply(p, Setting::LowImpedance(true), false).is_err());
            } else {
                c.apply(p, Setting::Phantom(true), true).unwrap();
                let offset = if mk2 { 1 } else { 6 };
                for i in 0..p.length {
                    if i != offset {
                        assert_eq!(c.0[0].1[i], original.0[0].1[i]);
                    }
                }
                if mk2 {
                    assert_eq!(c.0[0].1[1], 0xa6);
                    assert_eq!(c.0[1..], original.0[1..]);
                }
            }
            for bad in [f64::NAN, f64::INFINITY, -1.0, p.max_gain + 1.0] {
                assert!(c.apply(p, Setting::Gain(bad), false).is_err());
            }
            c.apply(p, Setting::Gain(37.5), false).unwrap();
            let target = Target {
                profile: p,
                bus: 1,
                address: 1,
                serial: "test".into(),
                token: "test".into(),
            };
            assert_eq!(c.controls(&target).gain_db, if mk2 { 38.0 } else { 37.5 });
            c.apply(p, Setting::Headphones(-12.5), false).unwrap();
            assert_eq!(c.controls(&target).headphone_db, -12.5);
            if mk2 || p.mix_offset.is_some() {
                c.apply(p, Setting::MonitorMix(75.0), false).unwrap();
                assert_eq!(c.controls(&target).monitor_mix, Some(75.0));
            } else {
                assert!(c.apply(p, Setting::MonitorMix(75.0), false).is_err());
            }
        }
    }

    #[test]
    fn banks_require_every_complete_block_and_never_fallback_on_timeout() {
        let (bank, _) = detect_bank(|bank, _, length| {
            Ok(vec![
                0;
                if bank == 0x0103 && length == 2 {
                    1
                } else {
                    length
                }
            ])
        })
        .unwrap();
        assert_eq!(bank, 0x0203);
        let mut calls = 0;
        assert!(detect_bank(|_, _, _| {
            calls += 1;
            Err(rusb::Error::Timeout)
        })
        .is_err());
        assert_eq!(calls, 1);
        assert!(detect_bank(|_, _, length| Ok(vec![0; length - 1])).is_err());
    }

    #[test]
    #[ignore = "Read-only physical acceptance: WAVESINK_ELGATO_TEST_CARD must identify the connected unit"]
    fn physical_read_only() {
        let card = std::env::var("WAVESINK_ELGATO_TEST_CARD")
            .unwrap()
            .parse()
            .unwrap();
        let controls = read(card).unwrap().unwrap();
        assert_eq!(read(card).unwrap().unwrap().token, controls.token);
        println!("{}", serde_json::to_string_pretty(&controls).unwrap());
    }

    #[test]
    #[ignore = "Physical gain write/restore: requires explicit card and exact serial environment variables"]
    fn physical_gain_round_trip() {
        let card = std::env::var("WAVESINK_ELGATO_TEST_CARD")
            .unwrap()
            .parse()
            .unwrap();
        let serial = std::env::var("WAVESINK_ELGATO_TEST_SERIAL").unwrap();
        let before = read(card).unwrap().unwrap();
        assert_eq!(before.serial, serial);
        assert!(!serial.is_empty());
        let step = if before.model == "XLR Dock MK.2" {
            1.0
        } else {
            0.5
        };
        let gain = if before.gain_db >= step {
            before.gain_db - step
        } else {
            before.gain_db + step
        };
        let changed = write(card, &before.token, Setting::Gain(gain), false);
        // Always attempt restoration before asserting the transfer result.
        let restored = write(card, &before.token, Setting::Gain(before.gain_db), false)
            .expect("Could not restore gain; inspect hardware state");
        assert_eq!(restored.gain_db, before.gain_db);
        let changed = changed.unwrap();
        assert_eq!(changed.gain_db, gain);
        for observed in [&changed, &restored] {
            assert_eq!(observed.phantom, before.phantom);
            assert_eq!(observed.muted, before.muted);
            assert_eq!(observed.headphone_db, before.headphone_db);
            assert_eq!(observed.low_impedance, before.low_impedance);
            assert_eq!(observed.monitor_mix, before.monitor_mix);
        }
        println!(
            "{} gain {} -> {} -> {} dB; phantom unchanged",
            before.model, before.gain_db, changed.gain_db, restored.gain_db
        );
    }
}
