pub mod error;
pub mod ioctl;

#[cfg(test)]
mod tests;

use byteorder::{BigEndian, ReadBytesExt};
use error::{DroboError, Result};
use ioctl::DroboIOctl;
use rand::Rng;
use std::fs;
use std::io::Read;
use std::path::Path;

pub const VERSION: &str = "0.1.0";
pub const MAX_TRANSACTION: u32 = 250;

#[derive(Debug, Clone)]
pub struct ConfigInfo {
    pub max_slots: u8,
    pub max_luns: u8,
    pub max_lun_size_bytes: u64,
}

#[derive(Debug, Clone)]
pub struct CapacityInfo {
    pub free_bytes: u64,
    pub used_bytes: u64,
    pub virtual_bytes: u64,
    pub unprotected_bytes: u64,
}

#[derive(Debug, Clone)]
pub struct SlotInfo {
    pub slot_id: u16,
    pub physical_capacity: u64,
    pub managed_capacity: u64,
    pub leds: String,
    pub manufacturer: String,
    pub model: String,
}

#[derive(Debug, Clone)]
pub struct LunInfo {
    pub id: u16,
    pub total_capacity: u64,
    pub used_capacity: u64,
    pub partition_scheme: String,
    pub format: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct FirmwareInfo {
    pub major: u8,
    pub minor: u8,
    pub build: u16,
    pub upg_major: u8,
    pub upg_minor: u8,
    pub build_date: String,
    pub arch: String,
    pub version: String,
    pub features: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct DroboOptions {
    pub yellow_threshold: u8,
    pub red_threshold: u8,
    pub dual_disk_redundancy: bool,
    pub spin_down_delay_minutes: u16,
    pub spin_down_delay: bool,
    pub use_manual_volume_management: bool,
    pub use_static_ip_address: bool,
    pub ip_address: String,
    pub netmask: String,
}

pub struct Drobo {
    pub char_dev_file: String,
    pub char_devs: Vec<String>,
    pub features: Vec<String>,
    pub transaction_id: u32,
    pub simulation: bool,
    pub slot_count: u8,
    pub firmware: Option<FirmwareInfo>,
    ioctl: Option<DroboIOctl>,
}

pub fn discover_luns(vendor_string: Option<&str>) -> Vec<Vec<String>> {
    let mut devices = Vec::new();
    let devdir = "/dev";

    let read_res = fs::read_dir(devdir);
    if read_res.is_err() {
        return devices;
    }

    let mut potentials = Vec::new();
    for entry_res in read_res.unwrap() {
        if let Ok(entry) = entry_res {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with("sd") && name.len() == 3 {
                potentials.push(entry.path().to_string_lossy().to_string());
            }
        }
    }
    potentials.sort();

    let vendor_filter = vendor_string.unwrap_or("Drobo").to_lowercase();
    let mut current_lun_group = Vec::new();
    let mut previous_dev_id = String::new();

    for dev_path in potentials {
        if let Ok(pdio) = DroboIOctl::open(&dev_path) {
            if let Ok(id) = pdio.identify_lun() {
                let this_dev = format!("{:02}{:02}{:02}", id.0, id.1, id.2);
                let vendor_lower = id.4.to_lowercase();

                if vendor_lower.starts_with("trusted")
                    || vendor_lower.starts_with("drobo")
                    || vendor_lower.starts_with(&vendor_filter)
                {
                    if this_dev == previous_dev_id {
                        current_lun_group.push(dev_path);
                    } else {
                        if !current_lun_group.is_empty() {
                            devices.push(current_lun_group);
                        }
                        current_lun_group = vec![dev_path];
                    }
                    previous_dev_id = this_dev;
                }
            }
        }
    }

    if !current_lun_group.is_empty() {
        devices.push(current_lun_group);
    }

    devices
}

fn detect_host_partition_and_fs(dev_path: &str) -> (String, Vec<String>) {
    let mut scheme = "No Partitions".to_string();
    let mut formats = Vec::new();

    // Check blkid output for main device and partition 1 (e.g. /dev/sda and /dev/sda1)
    let p1 = format!("{}1", dev_path);
    let check_paths = [dev_path, &p1];

    for path in check_paths {
        if let Ok(output) = std::process::Command::new("blkid").arg(path).output() {
            let out_str = String::from_utf8_lossy(&output.stdout).to_string();
            if out_str.contains("PTTYPE=\"gpt\"") {
                scheme = "GPT".to_string();
            } else if out_str.contains("PTTYPE=\"dos\"") || out_str.contains("PTTYPE=\"mbr\"") {
                scheme = "MBR".to_string();
            }

            if out_str.contains("TYPE=\"ext") {
                if !formats.contains(&"EXT3".to_string()) && !formats.contains(&"EXT4".to_string()) {
                    if out_str.contains("ext4") {
                        formats.push("EXT4".to_string());
                    } else {
                        formats.push("EXT3".to_string());
                    }
                }
            } else if out_str.contains("TYPE=\"ntfs\"") {
                if !formats.contains(&"NTFS".to_string()) {
                    formats.push("NTFS".to_string());
                }
            } else if out_str.contains("TYPE=\"vfat\"") || out_str.contains("TYPE=\"fat\"") {
                if !formats.contains(&"FAT32".to_string()) {
                    formats.push("FAT32".to_string());
                }
            }
        }
    }

    // Also check /proc/partitions to see if partition 1 exists
    if scheme == "No Partitions" {
        if let Ok(content) = fs::read_to_string("/proc/partitions") {
            let p1_name = p1.trim_start_matches("/dev/");
            if content.contains(p1_name) {
                scheme = "GPT".to_string(); // Default to GPT for partition existence
            }
        }
    }

    (scheme, formats)
}

pub fn decode_diagnostics(data: &[u8]) -> String {
    if data.is_empty() {
        return String::new();
    }

    // Python Drobo.py:
    // key = ord(data[0]) ^ 0x2d
    // datam = ''.join([chr(ord(x) ^ key) for x in data])
    let key = data[0] ^ b'-';
    let decoded: Vec<u8> = data.iter().map(|&b| b ^ key).collect();
    String::from_utf8_lossy(&decoded).to_string()
}

pub fn part_scheme(n: u8) -> String {
    match n {
        0 => "No Partitions".to_string(),
        1 => "MBR".to_string(),
        2 => "APM".to_string(),
        3 => "GPT".to_string(),
        _ => format!("Unknown ({})", n),
    }
}

pub fn part_format(n: u8) -> Vec<String> {
    let mut f = Vec::new();
    if n & 0x01 != 0 { f.push("NO FORMAT".to_string()); }
    if n & 0x02 != 0 { f.push("NTFS".to_string()); }
    if n & 0x04 != 0 { f.push("HFS".to_string()); }
    if n & 0x80 != 0 || n & 0x08 != 0 { f.push("EXT3".to_string()); }
    if (n & 0xFF) == 0 { f.push("FAT32".to_string()); }
    f
}

pub fn led_status(n: u8) -> String {
    if n == 0x80 {
        return "gray".to_string();
    }
    let colors = [
        "black",
        "red",
        "yellow",
        "green",
        "red-green",
        "red-yellow",
        "red-black",
    ];
    let idx = (n & 0x0f) as usize;
    if idx < colors.len() {
        colors[idx].to_string()
    } else {
        "unknown".to_string()
    }
}

pub fn unit_status(n: u32) -> Vec<String> {
    let mut flags = Vec::new();
    if n & 0x0002 != 0 { flags.push("Red alert".to_string()); }
    if n & 0x0004 != 0 { flags.push("Yellow alert".to_string()); }
    if n & 0x0008 != 0 { flags.push("No disks".to_string()); }
    if n & 0x0010 != 0 { flags.push("Bad disk".to_string()); }
    if n & 0x0020 != 0 { flags.push("Too many missing disks".to_string()); }
    if n & 0x0040 != 0 { flags.push("No redundancy".to_string()); }
    if n & 0x0080 != 0 { flags.push("No magic hotspare".to_string()); }
    if n & 0x0100 != 0 { flags.push("No space left".to_string()); }
    if n & 0x0200 != 0 { flags.push("Relay out in progress".to_string()); }
    if n & 0x0400 != 0 { flags.push("Format in progress".to_string()); }
    if n & 0x0800 != 0 { flags.push("Mismatched disks".to_string()); }
    if n & 0x1000 != 0 { flags.push("Unknown version".to_string()); }
    if n & 0x2000 != 0 { flags.push("New firmware installed".to_string()); }
    if n & 0x4000 != 0 { flags.push("New LUN available after reboot".to_string()); }
    if n & 0x10000000 != 0 { flags.push("Unknown error".to_string()); }
    flags
}

pub fn unit_features(norig: u32) -> Vec<String> {
    let feature_map: &[(u32, &str)] = &[
        (0x0001, "NO_AUTO_REBOOT"),
        (0x0002, "NO_FAT32_FORMAT"),
        (0x0004, "USED_CAPACITY_FROM_HOST"),
        (0x0008, "DISKPACKSTATUS"),
        (0x0010, "ENCRYPT_NOHEADER"),
        (0x0020, "CMD_STATUS_QUERIABLE"),
        (0x0040, "VARIABLE_LUN_SIZE_1_16"),
        (0x0080, "PARTITION_LUN_GPT_MBR"),
        (0x0100, "FAT32_FORMAT_VOLNAME"),
        (0x0200, "SUPPORTS_DROBOSHARE"),
        (0x0400, "SUPPORTS_NEW_LUNINFO2"),
        (0x0800, "feature x0800"),
        (0x1000, "LUN_MANAGEMENT"),
        (0x2000, "feature x2000"),
        (0x4000, "SUPPORTS_OPTIONS2"),
        (0x8000, "SUPPORTS_SHUTDOWN"),
        (0x10000, "feature x10000"),
        (0x20000, "SUPPORTS_ISCSI"),
        (0x40000, "feature x40000"),
        (0x80000, "feature x80000"),
        (0x40000000, "SUPPORTS_VOLUME_RENAME"),
        (0x80000000, "SUPPORTS_SINGLE_LUN_FORMAT"),
    ];

    let mut f = Vec::new();
    let mut n = norig;
    for &(mask, label) in feature_map {
        if n & mask != 0 {
            n &= !mask;
            f.push(label.to_string());
        }
    }
    if n != 0 {
        f.push(format!("leftovers ({:#04x})", n));
    }
    f
}

impl Drobo {
    pub fn new(char_devs: Vec<String>, simulation: bool) -> Result<Self> {
        let char_dev_file = char_devs.first().cloned().unwrap_or_default();

        let mut rng = rand::thread_rng();
        let transaction_id = rng.gen_range(1..=MAX_TRANSACTION);

        let mut d = Self {
            char_dev_file,
            char_devs,
            features: Vec::new(),
            transaction_id,
            simulation,
            slot_count: 4,
            firmware: None,
            ioctl: None,
        };

        if !simulation {
            let ioctl = DroboIOctl::open(&d.char_dev_file)?;
            d.ioctl = Some(ioctl);

            let cfg = d.get_subpage_config()?;
            d.slot_count = cfg.max_slots;

            if d.slot_count < 4 {
                return Err(DroboError::InvalidDevice(
                    "All Drobos have at least 4 slots".to_string(),
                ));
            }

            d.firmware = Some(d.get_subpage_firmware()?);
        } else {
            d.slot_count = 8;
            d.features = vec![
                "NO_AUTO_REBOOT".to_string(),
                "NO_FAT32_FORMAT".to_string(),
                "SUPPORTS_OPTIONS2".to_string(),
                "SUPPORTS_NEW_LUNINFO2".to_string(),
            ];
            d.firmware = Some(FirmwareInfo {
                major: 1,
                minor: 201,
                build: 12942,
                upg_major: 12,
                upg_minor: 6,
                build_date: "May 13 2008,15:29:32".to_string(),
                arch: "ArmMarvell".to_string(),
                version: "1.1.2".to_string(),
                features: d.features.clone(),
            });
        }

        Ok(d)
    }

    fn transaction_next(&mut self) {
        if self.transaction_id >= MAX_TRANSACTION {
            self.transaction_id = 0;
        }
        self.transaction_id += 1;
    }

    fn get_subpage(&self, sub_page: u8, pack_len: usize) -> Result<Vec<u8>> {
        if self.simulation {
            return Ok(vec![0u8; pack_len]);
        }

        let full_len = 4 + pack_len;
        let mcb = vec![
            0x5a,
            0x00,
            0x3a,
            sub_page,
            0x00,
            0x00,
            0x00,
            (full_len >> 8) as u8,
            (full_len & 0xff) as u8,
            0x00,
        ];

        let ioctl = self.ioctl.as_ref().ok_or(DroboError::IoctlError)?;
        let res = ioctl.get_sub_page(full_len, &mcb, false)?;

        if res.len() < 4 {
            return Err(DroboError::UnexpectedLength {
                expected: full_len,
                got: res.len(),
            });
        }

        Ok(res[4..].to_vec())
    }

    pub fn get_subpage_config(&self) -> Result<ConfigInfo> {
        if self.simulation {
            return Ok(ConfigInfo {
                max_slots: 8,
                max_luns: 16,
                max_lun_size_bytes: 2199023250944,
            });
        }

        let bytes = self.get_subpage(0x01, 11)?;
        if bytes.len() < 11 {
            return Ok(ConfigInfo {
                max_slots: 4,
                max_luns: 1,
                max_lun_size_bytes: 0,
            });
        }
        let mut rdr = &bytes[..];

        let slots = rdr.read_u8().unwrap_or(4);
        let _reserved = rdr.read_u8().unwrap_or(0);
        let luns = rdr.read_u8().unwrap_or(1);
        let max_lun_sz = rdr.read_u64::<BigEndian>().unwrap_or(0);

        Ok(ConfigInfo {
            max_slots: slots,
            max_luns: luns,
            max_lun_size_bytes: max_lun_sz * 512,
        })
    }

    pub fn get_subpage_capacity(&self) -> Result<CapacityInfo> {
        if self.simulation {
            let cap = 495452160000;
            let used = 120405000000;
            return Ok(CapacityInfo {
                free_bytes: cap - used,
                used_bytes: used,
                virtual_bytes: cap,
                unprotected_bytes: 125184245760,
            });
        }

        let bytes = match self.get_subpage(0x02, 32) {
            Ok(b) => b,
            Err(_) => return Ok(CapacityInfo { free_bytes: 0, used_bytes: 0, virtual_bytes: 0, unprotected_bytes: 0 }),
        };
        if bytes.len() < 32 {
            return Ok(CapacityInfo { free_bytes: 0, used_bytes: 0, virtual_bytes: 0, unprotected_bytes: 0 });
        }
        let mut rdr = &bytes[..];

        let free_b = rdr.read_u64::<BigEndian>().unwrap_or(0);
        let used_b = rdr.read_u64::<BigEndian>().unwrap_or(0);
        let virt_b = rdr.read_u64::<BigEndian>().unwrap_or(0);
        let unprot_b = rdr.read_u64::<BigEndian>().unwrap_or(0);

        Ok(CapacityInfo {
            free_bytes: free_b,
            used_bytes: used_b,
            virtual_bytes: virt_b,
            unprotected_bytes: unprot_b,
        })
    }

    pub fn get_subpage_slot_info(&self) -> Result<Vec<SlotInfo>> {
        if self.simulation {
            return Ok(vec![
                SlotInfo {
                    slot_id: 0,
                    physical_capacity: 500107862016,
                    managed_capacity: 0,
                    leds: "green".to_string(),
                    manufacturer: "ST3500830AS".to_string(),
                    model: "ST3500830AS".to_string(),
                },
                SlotInfo {
                    slot_id: 1,
                    physical_capacity: 750156374016,
                    managed_capacity: 0,
                    leds: "green".to_string(),
                    manufacturer: "WDC".to_string(),
                    model: "WD7500AAKS".to_string(),
                },
                SlotInfo {
                    slot_id: 2,
                    physical_capacity: 0,
                    managed_capacity: 0,
                    leds: "gray".to_string(),
                    manufacturer: "".to_string(),
                    model: "".to_string(),
                },
                SlotInfo {
                    slot_id: 3,
                    physical_capacity: 0,
                    managed_capacity: 0,
                    leds: "gray".to_string(),
                    manufacturer: "".to_string(),
                    model: "".to_string(),
                },
            ]);
        }

        // slotrec: 'HBQQB32s16sL' => 2 + 1 + 8 + 8 + 1 + 32 + 16 + 4 = 72 bytes per slot
        let record_size = 72;
        let count = self.slot_count.max(4) as usize;
        let request_len = 1 + record_size * count;
        let bytes = match self.get_subpage(0x03, request_len) {
            Ok(b) => b,
            Err(_) => return Ok(Vec::new()),
        };

        let mut slots = Vec::new();

        for slot_idx in 0..count {
            let offset = 1 + slot_idx * record_size;
            if offset >= bytes.len() {
                // If subpage returned fewer bytes, report empty slot for missing records
                slots.push(SlotInfo {
                    slot_id: slot_idx as u16,
                    physical_capacity: 0,
                    managed_capacity: 0,
                    leds: "gray".to_string(),
                    manufacturer: "".to_string(),
                    model: "".to_string(),
                });
                continue;
            }

            let end = (offset + record_size).min(bytes.len());
            let mut chunk = vec![0u8; record_size];
            let available = end.saturating_sub(offset);
            chunk[..available].copy_from_slice(&bytes[offset..end]);

            let mut rdr_id = &chunk[0..2];
            let _slot_id_raw = rdr_id.read_u16::<BigEndian>().unwrap_or(slot_idx as u16);

            let mut rdr_phys = &chunk[3..11];
            let phys_cap = rdr_phys.read_u64::<BigEndian>().unwrap_or(0);

            let mut rdr_mgd = &chunk[11..19];
            let mgd_cap = rdr_mgd.read_u64::<BigEndian>().unwrap_or(0);

            let led_byte = chunk[19];
            let leds = led_status(led_byte);

            let mfg = String::from_utf8_lossy(&chunk[20..52]).trim_matches('\0').trim().to_string();
            let model = String::from_utf8_lossy(&chunk[52..68]).trim_matches('\0').trim().to_string();

            slots.push(SlotInfo {
                slot_id: slot_idx as u16,
                physical_capacity: phys_cap,
                managed_capacity: mgd_cap,
                leds,
                manufacturer: mfg,
                model,
            });
        }

        Ok(slots)
    }

    pub fn get_subpage_firmware(&self) -> Result<FirmwareInfo> {
        if self.simulation {
            return Ok(FirmwareInfo {
                major: 1,
                minor: 201,
                build: 12942,
                upg_major: 12,
                upg_minor: 6,
                build_date: "May 13 2008,15:29:32".to_string(),
                arch: "ArmMarvell".to_string(),
                version: "1.1.2".to_string(),
                features: self.features.clone(),
            });
        }

        let bytes = self.get_subpage(0x08, 324)?;
        let mut rdr = &bytes[..];

        let major = rdr.read_u8().unwrap_or(0);
        let minor = rdr.read_u8().unwrap_or(0);
        let build = rdr.read_u16::<BigEndian>().unwrap_or(0);
        let upg_major = rdr.read_u8().unwrap_or(0);
        let upg_minor = rdr.read_u8().unwrap_or(0);

        let mut date_buf = [0u8; 32];
        let _ = rdr.read(&mut date_buf);
        let build_date = String::from_utf8_lossy(&date_buf).trim_matches('\0').trim().to_string();

        let mut arch_buf = [0u8; 16];
        let _ = rdr.read(&mut arch_buf);
        let arch = String::from_utf8_lossy(&arch_buf).trim_matches('\0').trim().to_string();

        let mut ver_buf = [0u8; 16];
        let _ = rdr.read(&mut ver_buf);
        let version = String::from_utf8_lossy(&ver_buf).trim_matches('\0').trim().to_string();

        let mut rest_buf = [0u8; 240];
        let _ = rdr.read(&mut rest_buf);

        let features = if rest_buf.len() >= 116 {
            let mut feat_rdr = &rest_buf[112..116];
            let feat_raw = feat_rdr.read_u32::<BigEndian>().unwrap_or(0);
            unit_features(feat_raw)
        } else {
            Vec::new()
        };

        Ok(FirmwareInfo {
            major,
            minor,
            build,
            upg_major,
            upg_minor,
            build_date,
            arch,
            version,
            features,
        })
    }

    pub fn get_subpage_luns(&self) -> Result<Vec<LunInfo>> {
        if self.simulation {
            return Ok(vec![LunInfo {
                id: 0,
                total_capacity: 2199023251456,
                used_capacity: 5092651008,
                partition_scheme: "GPT".to_string(),
                format: vec!["EXT3".to_string()],
            }]);
        }

        // Subpage 0x04 format: B + (HBQQ)*8
        // H: 2 bytes (record tag 17)
        // B: 1 byte (LUN ID)
        // Q: 8 bytes (total cap)
        // Q: 8 bytes (used cap)
        let bytes_l04 = match self.get_subpage(0x04, 153) {
            Ok(b) => b,
            Err(_) => return Ok(Vec::new()),
        };
        if bytes_l04.is_empty() {
            return Ok(Vec::new());
        }

        let lun_count = (bytes_l04[0] as usize).min(8);
        let mut luns_base = Vec::new();
        let record_size_l04 = 19;

        for i in 0..lun_count {
            let offset = 1 + i * record_size_l04;
            if offset + record_size_l04 > bytes_l04.len() {
                break;
            }
            let chunk = &bytes_l04[offset..offset + record_size_l04];
            let lun_id = chunk[2] as u16;
            let total_cap = u64::from_be_bytes([
                chunk[3], chunk[4], chunk[5], chunk[6], chunk[7], chunk[8], chunk[9], chunk[10],
            ]);
            let used_cap = u64::from_be_bytes([
                chunk[11], chunk[12], chunk[13], chunk[14], chunk[15], chunk[16], chunk[17], chunk[18],
            ]);
            luns_base.push((lun_id, total_cap, used_cap));
        }

        // Check if SUPPORTS_NEW_LUNINFO2 is present or attempt subpage 0x07 directly
        let has_l2 = self.features.iter().any(|s| s == "SUPPORTS_NEW_LUNINFO2")
            || self.firmware.as_ref().map_or(false, |f| f.features.iter().any(|s| s == "SUPPORTS_NEW_LUNINFO2"));

        if has_l2 && lun_count > 0 {
            // Python LUNINFO2: "B" + "HBQBBBH3B" * 8
            // record_size = 19 bytes
            let record_size_l2 = 19;
            let request_len = 1 + record_size_l2 * lun_count;
            if let Ok(bytes_l07) = self.get_subpage(0x07, request_len) {
                if !bytes_l07.is_empty() {
                    let mut result = Vec::new();
                    for (idx, base) in luns_base.into_iter().enumerate() {
                        let offset = 1 + idx * record_size_l2;
                        if offset + record_size_l2 <= bytes_l07.len() {
                            let chunk = &bytes_l07[offset..offset + record_size_l2];
                            let _tag = u16::from_be_bytes([chunk[0], chunk[1]]);
                            let lun_id = chunk[2] as u16;
                            let total_cap_l2 = u64::from_be_bytes([
                                chunk[3], chunk[4], chunk[5], chunk[6], chunk[7], chunk[8], chunk[9], chunk[10],
                            ]);
                            let scheme_code = chunk[11];
                            let format_code = chunk[13];

                            let mut scheme = part_scheme(scheme_code);
                            let mut formats = part_format(format_code);

                            // If Drobo hardware reports No Partitions or NO FORMAT, fallback to Linux host OS inspection (blkid/proc)
                            let needs_format_fallback = formats.is_empty() || formats.contains(&"NO FORMAT".to_string());
                            if (scheme == "No Partitions" || needs_format_fallback) && !self.char_dev_file.is_empty() {
                                let (host_scheme, host_formats) = detect_host_partition_and_fs(&self.char_dev_file);
                                if scheme == "No Partitions" && host_scheme != "No Partitions" {
                                    scheme = host_scheme;
                                }
                                if needs_format_fallback && !host_formats.is_empty() {
                                    formats = host_formats;
                                }
                            }

                            result.push(LunInfo {
                                id: lun_id,
                                total_capacity: if total_cap_l2 > 0 { total_cap_l2 } else { base.1 },
                                used_capacity: base.2,
                                partition_scheme: scheme,
                                format: formats,
                            });
                        } else {
                            result.push(LunInfo {
                                id: base.0,
                                total_capacity: base.1,
                                used_capacity: base.2,
                                partition_scheme: "No Partitions".to_string(),
                                format: Vec::new(),
                            });
                        }
                    }
                    return Ok(result);
                }
            }
        }

        Ok(luns_base
            .into_iter()
            .map(|(id, total, used)| LunInfo {
                id,
                total_capacity: total,
                used_capacity: used,
                partition_scheme: "Unknown".to_string(),
                format: Vec::new(),
            })
            .collect())
    }

    pub fn get_subpage_settings(&self) -> Result<(u32, u16, String)> {
        if self.simulation {
            return Ok((1220112079, 8, "TRUSTED DATA".to_string()));
        }

        // Subpage 0x05: LH32s => 4 + 2 + 32 = 38 bytes payload
        let bytes = match self.get_subpage(0x05, 38) {
            Ok(b) => b,
            Err(_) => return Ok((0, 0, "Drobo".to_string())),
        };
        if bytes.len() < 38 {
            return Ok((0, 0, "Drobo".to_string()));
        }

        let mut rdr = &bytes[..];
        let utc = rdr.read_u32::<BigEndian>().unwrap_or(0);
        let offset = rdr.read_u16::<BigEndian>().unwrap_or(0);
        let mut name_buf = [0u8; 32];
        let _ = rdr.read(&mut name_buf);
        let name = String::from_utf8_lossy(&name_buf).trim_matches('\0').trim().to_string();

        Ok((utc, offset, if name.is_empty() { "Drobo".to_string() } else { name }))
    }

    pub fn get_diag_record(&mut self, diagcode: u8) -> Result<Vec<u8>> {
        if self.simulation {
            return Ok(b"SIMULATED DIAGNOSTICS RECORD\n".to_vec());
        }

        let buflen = 4096;
        let mut diags = Vec::new();
        let ioctl = self.ioctl.as_ref().ok_or(DroboError::IoctlError)?;

        let mut first = true;
        loop {
            let flags = if first { (0x01 << 5) | 0x01 } else { 0x01 };
            first = false;

            let mcb = vec![
                0xea,
                0x10,
                0x80,
                diagcode,
                0x00,
                self.transaction_id as u8,
                flags as u8,
                (buflen >> 8) as u8,
                (buflen & 0xff) as u8,
                0x00,
            ];

            let chunk = match ioctl.get_sub_page(buflen, &mcb, false) {
                Ok(c) => c,
                Err(_) => break,
            };

            let clen = chunk.len();
            diags.extend_from_slice(&chunk);

            if clen < buflen {
                break;
            }
        }

        Ok(diags)
    }

    pub fn dump_diagnostics(&mut self) -> Result<String> {
        let _ = self.rename(&"Drobo".to_string()); // Sync time

        use std::time::{SystemTime, UNIX_EPOCH};
        let secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| DroboError::CommandFailed(e.to_string()))?
            .as_secs();

        // Calculate UTC date/time parts (YYYY, MM, DD, HH, MM)
        let days = secs / 86400;
        let day_secs = secs % 86400;
        let hour = day_secs / 3600;
        let minute = (day_secs % 3600) / 60;

        // Days since Jan 1 1970 UTC calculation
        let z = (days as i64) + 719468;
        let era = (if z >= 0 { z } else { z - 146096 }) / 146097;
        let doe = (z - era * 146097) as u64;
        let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
        let year = (yoe as i64) + era * 400;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let day = doy - (153 * mp + 2) / 5 + 1;
        let month = if mp < 10 { mp + 3 } else { mp - 9 };
        let year = if month <= 2 { year + 1 } else { year };

        let mut rng = rand::thread_rng();
        let rand_id: u16 = rng.gen_range(0x1000..=0xffff);

        let path_str = format!(
            "/tmp/DroboDiag_{:04}_{:02}_{:02}_{:02}_{:02}_{:04x}.log",
            year, month, day, hour, minute, rand_id
        );
        let path = Path::new(&path_str);

        let mut data = Vec::new();
        if let Ok(d4) = self.get_diag_record(4) {
            data.extend_from_slice(&d4);
        }
        self.transaction_next();
        if let Ok(d7) = self.get_diag_record(7) {
            data.extend_from_slice(&d7);
        }

        fs::write(path, &data)?;
        Ok(path_str)
    }

    pub fn get_options(&self) -> Result<DroboOptions> {
        if self.simulation {
            return Ok(DroboOptions {
                yellow_threshold: 85,
                red_threshold: 95,
                dual_disk_redundancy: true,
                spin_down_delay_minutes: 15,
                spin_down_delay: true,
                use_manual_volume_management: false,
                use_static_ip_address: false,
                ip_address: "192.168.1.100".to_string(),
                netmask: "255.255.255.0".to_string(),
            });
        }

        let mut opts = DroboOptions {
            yellow_threshold: 85,
            red_threshold: 95,
            dual_disk_redundancy: false,
            spin_down_delay_minutes: 15,
            spin_down_delay: true,
            use_manual_volume_management: false,
            use_static_ip_address: false,
            ip_address: "0.0.0.0".to_string(),
            netmask: "255.255.255.0".to_string(),
        };

        // Subpage 0x30: BBBIBB => 1+1+1+4+1+1 = 9 bytes
        if let Ok(b30) = self.get_subpage(0x30, 9) {
            if b30.len() >= 2 {
                opts.yellow_threshold = b30[0];
                opts.red_threshold = b30[1];
            }
        }

        // Subpage 0x31 (Options2): QHLLLHLLLH => 8+2 + 4+4+4+2 + 4+4+4+2 = 42 bytes
        if let Ok(b31) = self.get_subpage(0x31, 42) {
            if b31.len() >= 10 {
                let flags = u64::from_be_bytes([
                    b31[0], b31[1], b31[2], b31[3], b31[4], b31[5], b31[6], b31[7],
                ]);
                let spin_mins = u16::from_be_bytes([b31[8], b31[9]]);

                opts.dual_disk_redundancy = (flags & 0x0001) != 0;
                opts.spin_down_delay = (flags & 0x0002) != 0;
                opts.spin_down_delay_minutes = spin_mins;
                opts.use_manual_volume_management = (flags & 0x0004) != 0;
                opts.use_static_ip_address = (flags & 0x0008) != 0;

                if b31.len() >= 18 {
                    let raw_ip = u32::from_be_bytes([b31[10], b31[11], b31[12], b31[13]]);
                    let raw_nm = u32::from_be_bytes([b31[14], b31[15], b31[16], b31[17]]);
                    opts.ip_address = format!(
                        "{}.{}.{}.{}",
                        (raw_ip >> 24) & 0xff,
                        (raw_ip >> 16) & 0xff,
                        (raw_ip >> 8) & 0xff,
                        raw_ip & 0xff
                    );
                    opts.netmask = format!(
                        "{}.{}.{}.{}",
                        (raw_nm >> 24) & 0xff,
                        (raw_nm >> 16) & 0xff,
                        (raw_nm >> 8) & 0xff,
                        raw_nm & 0xff
                    );
                }
            }
        }

        Ok(opts)
    }

    pub fn set_options(&mut self, options: &DroboOptions) -> Result<()> {
        if self.simulation {
            return Ok(());
        }

        // 1. Subpage 0x30 Options write:
        // Python format: buffer = struct.pack(">BBHBBBLBB", 0x7a, 0x30, payloadlen, yel, red, 0, 0, 0, 0)
        // payloadlen = struct.calcsize('BBBLBB') = 10 bytes
        let mut buffer30 = Vec::new();
        buffer30.push(0x7a);
        buffer30.push(0x30);
        buffer30.extend_from_slice(&10u16.to_be_bytes()); // payloadlen = 10
        buffer30.push(options.yellow_threshold);
        buffer30.push(options.red_threshold);
        buffer30.push(0x00);
        buffer30.extend_from_slice(&0u32.to_be_bytes()); // L (4 bytes)
        buffer30.push(0x00);
        buffer30.push(0x00);

        let sblen30 = buffer30.len();
        let mcb30 = vec![
            0x55,
            0x01,
            0x7a,
            0x30,
            0x00,
            self.transaction_id as u8,
            0x01 << 5,
            (sblen30 >> 8) as u8,
            (sblen30 & 0xff) as u8,
            0x00,
        ];
        if let Some(ref ioctl) = self.ioctl {
            let _ = ioctl.put_sub_page(&mcb30, &buffer30);
        }
        self.transaction_next();

        // 2. Subpage 0x31 Options2 write (if supported):
        let has_opts2 = self.features.iter().any(|s| s == "SUPPORTS_OPTIONS2")
            || self.firmware.as_ref().map_or(false, |f| f.features.iter().any(|s| s == "SUPPORTS_OPTIONS2"));

        if has_opts2 {
            let mut flags: u64 = 0;
            if options.dual_disk_redundancy { flags |= 0x0001; }
            if options.spin_down_delay_minutes > 0 { flags |= 0x0002; }
            if options.use_manual_volume_management { flags |= 0x0004; }
            if options.use_static_ip_address { flags |= 0x0008; }

            let parse_ip = |s: &str| -> u32 {
                let parts: Vec<u8> = s.split('.').filter_map(|p| p.parse().ok()).collect();
                if parts.len() == 4 {
                    ((parts[0] as u32) << 24) | ((parts[1] as u32) << 16) | ((parts[2] as u32) << 8) | (parts[3] as u32)
                } else {
                    0
                }
            };

            let ip_num = parse_ip(&options.ip_address);
            let nm_num = parse_ip(&options.netmask);

            let mut buffer31 = Vec::new();
            buffer31.push(0x7a);
            buffer31.push(0x31);
            buffer31.extend_from_slice(&42u16.to_be_bytes()); // payloadlen = 42
            buffer31.extend_from_slice(&flags.to_be_bytes());
            buffer31.extend_from_slice(&options.spin_down_delay_minutes.to_be_bytes());
            buffer31.extend_from_slice(&ip_num.to_be_bytes());
            buffer31.extend_from_slice(&nm_num.to_be_bytes());
            buffer31.extend_from_slice(&0u32.to_be_bytes()); // Gateway
            buffer31.extend_from_slice(&1500u16.to_be_bytes()); // MTU
            buffer31.extend_from_slice(&ip_num.to_be_bytes());
            buffer31.extend_from_slice(&nm_num.to_be_bytes());
            buffer31.extend_from_slice(&0u32.to_be_bytes());
            buffer31.extend_from_slice(&1500u16.to_be_bytes());

            let sblen31 = buffer31.len();
            let mcb31 = vec![
                0x55,
                0x01,
                0x7a,
                0x31,
                0x00,
                self.transaction_id as u8,
                0x01 << 5,
                (sblen31 >> 8) as u8,
                (sblen31 & 0xff) as u8,
                0x00,
            ];
            if let Some(ref ioctl) = self.ioctl {
                let _ = ioctl.put_sub_page(&mcb31, &buffer31);
            }
            self.transaction_next();
        }

        Ok(())
    }

    pub fn format_script(&self, fstype: &str, script_path: &Path) -> Result<String> {
        let mut script = String::new();
        script.push_str("#!/bin/sh\n");
        script.push_str("# Drobo Format script generated by droboapi\n");
        script.push_str("set -e\n\n");

        let ptype = if fstype == "FAT32" || fstype == "msdos" {
            "msdos"
        } else {
            "gpt"
        };

        for cd in &self.char_devs {
            script.push_str(&format!("parted -s {} mklabel {}\n", cd, ptype));

            match fstype {
                "ext3" | "ext4" => {
                    script.push_str(&format!("parted -s {} mkpart primary ext3 0 100%\n", cd));
                    script.push_str(&format!("parted -s {} print; sleep 5\n", cd));
                    script.push_str(&format!(
                        "mke2fs -j -i 262144 -L Drobo01 -m 0 -O sparse_super,^resize_inode {}1\n",
                        cd
                    ));
                }
                "ntfs" => {
                    script.push_str(&format!("parted -s {} mkpart primary ntfs 0 100%\n", cd));
                    script.push_str(&format!("parted -s {} print; sleep 5\n", cd));
                    script.push_str(&format!("mkntfs -f -L Drobo01 {}1\n", cd));
                }
                "FAT32" | "msdos" => {
                    script.push_str(&format!("parted -s {} mkpart primary fat32 0 100%\n", cd));
                    script.push_str(&format!("parted -s {} print; sleep 5\n", cd));
                    script.push_str(&format!("mkdosfs -v -v -F 32 -S 4096 -n Drobo01 {}1\n", cd));
                }
                _ => {
                    return Err(DroboError::CommandFailed(format!(
                        "Unsupported filesystem type: {}",
                        fstype
                    )));
                }
            }
        }

        fs::write(script_path, &script)?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(script_path, fs::Permissions::from_mode(0o755));
        }

        Ok(script_path.to_string_lossy().to_string())
    }

    pub fn rename(&mut self, new_name: &str) -> Result<()> {
        if self.simulation {
            return Ok(());
        }

        use std::time::{SystemTime, UNIX_EPOCH};
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| DroboError::CommandFailed(e.to_string()))?
            .as_secs() as u32;

        let mut name_bytes = [0u8; 32];
        let bytes = new_name.as_bytes();
        let len = bytes.len().min(32);
        name_bytes[..len].copy_from_slice(&bytes[..len]);

        // Python Drobo.py Sync():
        // buffer = struct.pack(">BBHLH32s", 0x7a, 0x05, payloadlen, now, 0, NewName) => 2+1+2 + 4+2+32 = 43 bytes
        let mut buffer = Vec::new();
        buffer.push(0x7a);
        buffer.push(0x05);
        buffer.extend_from_slice(&38u16.to_be_bytes()); // payloadlen = 4+2+32 = 38
        buffer.extend_from_slice(&now.to_be_bytes());
        buffer.extend_from_slice(&0u16.to_be_bytes());
        buffer.extend_from_slice(&name_bytes);

        let sblen = buffer.len();
        let modepageblock = vec![
            0x55,
            0x01,
            0x7a,
            0x05,
            0x00,
            0x00,
            0x00,
            (sblen >> 8) as u8,
            (sblen & 0xff) as u8,
            0x00,
        ];

        let ioctl = self.ioctl.as_ref().ok_or(DroboError::IoctlError)?;
        ioctl.put_sub_page(&modepageblock, &buffer)?;
        self.transaction_next();
        Ok(())
    }

    pub fn blink(&mut self) -> Result<()> {
        self.issue_command(0x06)
    }

    pub fn standby(&mut self) -> Result<()> {
        self.issue_command(0x0d)
    }

    fn issue_command(&mut self, command: u8) -> Result<()> {
        if self.simulation {
            self.transaction_next();
            return Ok(());
        }

        // Python format >BBBBBBBHB:
        // B: 0xea
        // B: 0x10
        // B: 0x00
        // B: command (0x06 for blink)
        // B: 0x00
        // B: transaction_id
        // B: 0x01 << 5 (0x20)
        // H: 0x0001 (2 bytes: 0x00, 0x01)
        // B: 0x00
        let mcb = vec![
            0xea,
            0x10,
            0x00,
            command,
            0x00,
            self.transaction_id as u8,
            0x01 << 5,
            0x00, // High byte of 16-bit H (0x0001)
            0x01, // Low byte of 16-bit H (0x0001)
            0x00,
        ];

        let ioctl = self.ioctl.as_ref().ok_or(DroboError::IoctlError)?;
        
        // Python: cmdout = self.fd.get_sub_page(1, modepageblock, 1, DEBUG)
        let _ = ioctl.get_sub_page(1, &mcb, true);
        self.transaction_next();
        Ok(())
    }
}
