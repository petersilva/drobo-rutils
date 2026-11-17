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
            let settings = drobo.get_subpage_settings().unwrap_or((0, 0, "Drobo".to_string()));
            println!("Device: {} | Name: {}", drobo.char_dev_file, settings.2);

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
    }

    Ok(())
}
