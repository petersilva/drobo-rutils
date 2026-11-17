use clap::{Parser, Subcommand};
use droboapi::error::Result;
use droboapi::Drobo;

#[derive(Parser, Debug)]
#[command(author, version, about = "Drobo Management Interface in Rust", long_about = None)]
struct Cli {
    #[arg(short, long, help = "Specify device file path (e.g., /dev/sdc)")]
    device: Option<String>,

    #[arg(short, long, help = "Enable simulation mode for testing without hardware")]
    simulate: bool,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand, Debug)]
enum Commands {
    Blink,

    Info,

    Status,

    Standby,

    Rename {
        #[arg(help = "New name for the Drobo unit")]
        name: String,
    },

    Diags,

    DiagPrint {
        #[arg(help = "Path to diagnostic log file (e.g. /tmp/DroboDiag_*.log)")]
        file: String,
    },

    Options,

    Set {
        #[arg(help = "Setting key (e.g. name, lunsize, time, YellowThreshold, RedThreshold, DualDiskRedundancy, SpinDownDelayMinutes, IPAddress, NetMask)")]
        key: String,

        #[arg(help = "Value to set (optional for 'time')")]
        value: Option<String>,
    },
}

fn format_utc_timestamp(ts: u32) -> String {
    if ts == 0 {
        return "Not Set / Unknown".to_string();
    }
    let sec = ts as u64;
    let days = sec / 86400;
    let rem_sec = sec % 86400;
    let hours = rem_sec / 3600;
    let minutes = (rem_sec % 3600) / 60;
    let seconds = rem_sec % 60;

    // Civil calendar conversion algorithm (Jan 1 1970 epoch)
    let z = days + 719468;
    let era = z / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };

    format!("{:04}-{:02}-{:02} {:02}:{:02}:{:02} UTC (timestamp: {})", y, m, d, hours, minutes, seconds, ts)
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    let (devices, is_sim) = if let Some(dev) = cli.device {
        (vec![dev], cli.simulate)
    } else if cli.simulate {
        (vec!["/dev/simulated_drobo".to_string()], true)
    } else {
        println!("No device specified. Scanning for attached Drobo devices...");
        let discovered = droboapi::discover_luns(None);
        if discovered.is_empty() {
            println!("No Drobos discovered.");
            println!("Try running as root/sudo, specify a device (-d /dev/sdX), or use simulation mode (-s).");
            return Ok(());
        }
        (discovered[0].clone(), false)
    };

    let mut drobo = Drobo::new(devices, is_sim)?;

    match cli.command.unwrap_or(Commands::Info) {
        Commands::Blink => {
            println!("Requesting Drobo to blink lights...");
            drobo.blink()?;
            println!("Done.");
        }
        Commands::Info => {
            println!("---------------------------------------------------------");
            let settings = drobo.get_subpage_settings().unwrap_or((0, 0, "Drobo".to_string()));
            let devices_str = drobo.char_devs.join(":");
            println!("Drobo Name  : {:<15} Devices: {}", settings.2, devices_str);
            println!("Time        : {}", format_utc_timestamp(settings.0));
            println!("Year Offset : {}", settings.1);
            println!("---------------------------------------------------------");

            let config = drobo.get_subpage_config()?;
            println!("\n--- Drobo Configuration ---");
            println!("Max Slots: {}", config.max_slots);
            println!("Max LUNs: {}", config.max_luns);
            if config.max_lun_size_bytes > 0 {
                println!("Max LUN Size: {} TiB", config.max_lun_size_bytes / (1024 * 1024 * 1024 * 1024));
            }

            let cap = drobo.get_subpage_capacity()?;
            println!("\n--- Drobo Capacity ---");
            println!("Used: {:.2} GB", cap.used_bytes as f64 / 1e9);
            println!("Free: {:.2} GB", cap.free_bytes as f64 / 1e9);
            println!("Total Virtual: {:.2} GB", cap.virtual_bytes as f64 / 1e9);

            if let Ok(opts) = drobo.get_options() {
                println!("\n--- Drobo Options ---");
                println!("Yellow Threshold: {}%", opts.yellow_threshold);
                println!("Red Threshold: {}%", opts.red_threshold);
                println!("Dual Disk Redundancy: {}", opts.dual_disk_redundancy);
                println!("Spin Down Delay: {} mins (Enabled: {})", opts.spin_down_delay_minutes, opts.spin_down_delay);
                println!("Manual Volume Management: {}", opts.use_manual_volume_management);
                println!("Use Static IP: {}", opts.use_static_ip_address);
                if !opts.ip_address.is_empty() {
                    println!("IP Address: {}", opts.ip_address);
                }
                if !opts.netmask.is_empty() {
                    println!("NetMask: {}", opts.netmask);
                }
            }

            let luns = drobo.get_subpage_luns()?;
            if !luns.is_empty() {
                println!("\n--- LUN Details ---");
                for lun in luns {
                    let total_tib = lun.total_capacity as f64 / (1024.0 * 1024.0 * 1024.0 * 1024.0);
                    let used_gb = lun.used_capacity as f64 / 1e9;
                    println!(
                        "  LUN {}: {:.2} TiB total ({:.2} GB used) | Partition Scheme: {} | Format: {:?}",
                        lun.id, total_tib, used_gb, lun.partition_scheme, lun.format
                    );
                }
            }

            if let Some(fw) = &drobo.firmware {
                println!("\n--- Firmware Info ---");
                println!("Version: {}", fw.version);
                println!("Arch: {}", fw.arch);
                println!("Build Date: {}", fw.build_date);
                println!("Features: {:?}", fw.features);
            }
        }
        Commands::Status => {
            let cap = drobo.get_subpage_capacity()?;
            let pct = if cap.virtual_bytes > 0 {
                (cap.used_bytes as f64 / cap.virtual_bytes as f64) * 100.0
            } else {
                0.0
            };
            println!("Device: {} | Usage: {:.1}% full", drobo.char_dev_file, pct);

            let luns = drobo.get_subpage_luns()?;
            if !luns.is_empty() {
                println!("\nLUN Status:");
                for lun in luns {
                    let total_tib = lun.total_capacity as f64 / (1024.0 * 1024.0 * 1024.0 * 1024.0);
                    println!(
                        "  LUN {}: {:.2} TiB | Scheme: {} | Format: {:?}",
                        lun.id, total_tib, lun.partition_scheme, lun.format
                    );
                }
            }

            let slots = drobo.get_subpage_slot_info()?;
            println!("\nSlot Status:");
            for slot in slots {
                println!(
                    "  Slot {}: {:10} | {:15} | {:15}",
                    slot.slot_id, slot.leds, slot.manufacturer, slot.model
                );
            }
        }
        Commands::Standby => {
            println!("Sending standby/shutdown command to Drobo...");
            drobo.standby()?;
            println!("Shutdown initiated.");
        }
        Commands::Rename { name } => {
            println!("Renaming Drobo unit to '{}'...", name);
            drobo.rename(&name)?;
            println!("Drobo renamed successfully.");
        }
        Commands::Diags => {
            println!("Dumping diagnostics to /tmp...");
            let logpath = drobo.dump_diagnostics()?;
            println!("Diagnostics written to: {}", logpath);
        }
        Commands::DiagPrint { file } => {
            let content = std::fs::read(&file)?;
            let decoded = droboapi::decode_diagnostics(&content);
            println!("--- Decoded Diagnostics ({}) ---", file);
            println!("{}", decoded);
        }
        Commands::Options => {
            let opts = drobo.get_options()?;
            println!("--- Drobo Options ---");
            println!("Yellow Threshold: {}%", opts.yellow_threshold);
            println!("Red Threshold: {}%", opts.red_threshold);
            println!("Dual Disk Redundancy: {}", opts.dual_disk_redundancy);
            println!("Spin Down Delay: {} mins (Enabled: {})", opts.spin_down_delay_minutes, opts.spin_down_delay);
            println!("Manual Volume Management: {}", opts.use_manual_volume_management);
            println!("Use Static IP: {}", opts.use_static_ip_address);
            println!("IP Address: {}", opts.ip_address);
            println!("NetMask: {}", opts.netmask);
        }
        Commands::Set { key, value } => {
            let key_lower = key.to_lowercase();
            match key_lower.as_str() {
                "name" => {
                    let val = value.ok_or_else(|| {
                        droboapi::error::DroboError::CommandFailed("Missing value for 'set name <NAME>'".to_string())
                    })?;
                    println!("Setting Drobo unit name to '{}'...", val);
                    drobo.rename(&val)?;
                    println!("Name set successfully.");
                }
                "lunsize" => {
                    let val = value.ok_or_else(|| {
                        droboapi::error::DroboError::CommandFailed("Missing value for 'set lunsize <TB>'".to_string())
                    })?;
                    let tb: u32 = val.parse().map_err(|_| {
                        droboapi::error::DroboError::CommandFailed(format!("Invalid LUN size: '{}'. Must be an integer 1-16 TiB", val))
                    })?;
                    if !(1..=16).contains(&tb) {
                        return Err(droboapi::error::DroboError::CommandFailed(
                            "LUN size must be between 1 and 16 TiB.".to_string(),
                        ));
                    }
                    if tb > 2 {
                        println!("Warning: LUN sizes > 2 TiB may not be supported on older systems.");
                    }
                    println!("Setting LUN size to {} TiB...", tb);
                    drobo.set_lun_size(tb)?;
                    println!("LUN size set successfully.");
                }
                "time" => {
                    println!("Syncing Drobo clock with host time...");
                    drobo.sync_time()?;
                    println!("Clock synchronized successfully.");
                }
                _ => {
                    let val_str = value.as_deref().unwrap_or("");
                    let mut opts = drobo.get_options()?;
                    let mut matched = true;

                    match key_lower.as_str() {
                        "yellowthreshold" | "yellow_threshold" | "yellowalert" => {
                            let val: u8 = val_str.parse().map_err(|_| {
                                droboapi::error::DroboError::CommandFailed(format!("Invalid integer for {}: '{}'", key, val_str))
                            })?;
                            opts.yellow_threshold = val;
                        }
                        "redthreshold" | "red_threshold" | "redalert" => {
                            let val: u8 = val_str.parse().map_err(|_| {
                                droboapi::error::DroboError::CommandFailed(format!("Invalid integer for {}: '{}'", key, val_str))
                            })?;
                            opts.red_threshold = val;
                        }
                        "dualdiskredundancy" | "dual_disk_redundancy" | "redundancy" => {
                            let b = match val_str.to_lowercase().as_str() {
                                "true" | "1" | "yes" | "on" => true,
                                "false" | "0" | "no" | "off" => false,
                                _ => return Err(droboapi::error::DroboError::CommandFailed(format!("Invalid boolean for {}: '{}'", key, val_str))),
                            };
                            opts.dual_disk_redundancy = b;
                        }
                        "spindowndelayminutes" | "spin_down_delay_minutes" | "spindowndelay" | "spindown" => {
                            let mins: u16 = val_str.parse().map_err(|_| {
                                droboapi::error::DroboError::CommandFailed(format!("Invalid integer for {}: '{}'", key, val_str))
                            })?;
                            opts.spin_down_delay_minutes = mins;
                            opts.spin_down_delay = mins > 0;
                        }
                        "usemanualvolumemanagement" | "use_manual_volume_management" | "manualvolumemanagement" => {
                            let b = match val_str.to_lowercase().as_str() {
                                "true" | "1" | "yes" | "on" => true,
                                "false" | "0" | "no" | "off" => false,
                                _ => return Err(droboapi::error::DroboError::CommandFailed(format!("Invalid boolean for {}: '{}'", key, val_str))),
                            };
                            opts.use_manual_volume_management = b;
                        }
                        "usestaticipaddress" | "use_static_ip_address" | "usestaticip" => {
                            let b = match val_str.to_lowercase().as_str() {
                                "true" | "1" | "yes" | "on" => true,
                                "false" | "0" | "no" | "off" => false,
                                _ => return Err(droboapi::error::DroboError::CommandFailed(format!("Invalid boolean for {}: '{}'", key, val_str))),
                            };
                            opts.use_static_ip_address = b;
                        }
                        "ipaddress" | "ip_address" | "ip" => {
                            if val_str.is_empty() {
                                return Err(droboapi::error::DroboError::CommandFailed("Missing IP address value".to_string()));
                            }
                            opts.ip_address = val_str.to_string();
                        }
                        "netmask" | "net_mask" | "subnetmask" => {
                            if val_str.is_empty() {
                                return Err(droboapi::error::DroboError::CommandFailed("Missing netmask value".to_string()));
                            }
                            opts.netmask = val_str.to_string();
                        }
                        _ => {
                            matched = false;
                        }
                    }

                    if matched {
                        println!("Updating option '{}' to '{}'...", key, val_str);
                        drobo.set_options(&opts)?;
                        println!("Option updated successfully.");
                    } else {
                        return Err(droboapi::error::DroboError::CommandFailed(format!(
                            "Unknown setting key: '{}'. Valid keys: name, lunsize, time, YellowThreshold, RedThreshold, DualDiskRedundancy, SpinDownDelayMinutes, UseManualVolumeManagement, UseStaticIPAddress, IPAddress, NetMask",
                            key
                        )));
                    }
                }
            }
        }
    }

    Ok(())
}
