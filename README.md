# drobo-rutils

A high-performance, modern Rust rewrite of the `drobo-utils` management toolset for Data Robotics / Drobo storage units.
This project replaces Python scripts with a safe, memory-efficient Rust Cargo workspace composed of three distinct crates:

- **`droboapi`**: Safe library wrapper for Drobo SCSI generic (`sg`) `ioctl` communication, subpage decoding, and device auto-discovery.
- **`drobocli`**: Command-line utility replacing `drobom` with full subcommand support and `--simulate` testing flags.
- **`drobogui`**: Fast, reactive GUI dashboard built with `egui` / `eframe` replacing `droboview` and `DroboGUI.py`.

note from 2026:

I tested as well as I could, given the limitations. I still have the 18 year old original Drobo device, and the following
work with my unit:

 * all the status stuff, seems ok.
 * blinking
 * LUN size,
 * option setting.
 * diagnostic dump & display.
 
The upstream source for firmwares is gone, so I can't test firmware upgrades. As far as I can tell, aside from that,
it works as well as the python implementation. This seems like more moden approach.

---

## Workspace Architecture

```
drobo-rutils/
├── Cargo.toml            # Top-level workspace definition
├── droboapi/             # Core Library Crate
│   ├── src/
│   │   ├── lib.rs        # Drobo high-level API & subpage parsers
│   │   ├── ioctl.rs      # Low-level Linux SCSI SG_IO ioctl wrappers
│   │   └── error.rs      # Strongly-typed error enum (DroboError)
├── drobocli/             # CLI Binary Crate (drobom replacement)
│   └── src/main.rs       # Clap derive CLI interface & subcommands
└── drobogui/             # Desktop GUI Crate (droboview replacement)
    └── src/main.rs       # egui/eframe GUI tabs & diagnostic viewer
```

---

## Prerequisites & Building

### Requirements
- Rust 1.70+ and Cargo
- Linux operating system (with `/dev/sd*` or `/dev/sg*` SCSI generic support)

### Compiling
Build all workspace crates in release mode:
```bash
cargo build --release
```
The compiled binaries will be located in `./target/release/`:
- `./target/release/drobocli`
- `./target/release/drobogui`

---

## 1. `droboapi` Library Usage

`droboapi` provides safe abstractions for discovering and communicating with Drobo units over Linux SCSI ioctls.

### Example Usage:
```rust
use droboapi::{Drobo, discover_luns};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 1. Auto-discover attached Drobo devices
    let discovered = discover_luns(None);
    if discovered.is_empty() {
        println!("No Drobo hardware detected.");
        return Ok(());
    }

    // 2. Open Drobo device instance (e.g. /dev/sda)
    let mut drobo = Drobo::new(discovered[0].clone(), false)?;

    // 3. Query configuration & slot status
    let cfg = drobo.get_subpage_config()?;
    let slots = drobo.get_subpage_slot_info()?;
    println!("Max Slots: {}, Detected Slots: {}", cfg.max_slots, slots.len());

    // 4. Request unit action
    drobo.blink()?;

    Ok(())
}
```

---

## 2. `drobocli` (Command-Line Utility)

`drobocli` provides full management capabilities from the command line.

### Running as Root (Recommended for Hardware Access):
```bash
sudo ./target/release/drobocli [COMMAND]
```

### Available Commands:

| Subcommand | Description |
| :--- | :--- |
| `status` | Display connected Drobo device node, capacity usage, LUN details, and drive bay slot LEDs. |
| `info` | Show hardware configuration, LUN partitions/formats, and firmware version/features. |
| `blink` | Trigger the Drobo front-panel LED blinking light test. |
| `standby` | Issue a graceful shutdown / standby command to the Drobo. |
| `rename <NAME>` | Rename the Drobo unit volume label and sync host UTC time. |
| `options` | Query current thresholds, spin-down delay, redundancy, and network settings. |
| `diags` | Stream raw diagnostic records from the hardware and save to `/tmp/DroboDiag_*.log`. |
| `diagprint <FILE>` | Decrypt and display an encrypted diagnostic log file. |

### Global Flags:
- `-d, --device <PATH>`: Specify explicit device node (e.g., `/dev/sdc`).
- `-s, --simulate`: Run in non-intrusive hardware simulation mode (no root access required).

### Examples:
```bash
# Check status in simulation mode (no hardware required)
cargo run -p drobocli -- --simulate status

# Rename Drobo hardware to "BackupStore"
sudo ./target/release/drobocli rename "BackupStore"

# Dump hardware diagnostics to /tmp
sudo ./target/release/drobocli diags

# View decoded contents of a diagnostic log
./target/release/drobocli diagprint /tmp/DroboDiag_2026_11_16_14_42_a91f.log
```

---

## 3. `drobogui` (Desktop Dashboard)

`drobogui` provides a reactive graphical interface matching the original PyQt5 app layout.

### Launching:
```bash
sudo ./target/release/drobogui
```

### Dashboard Tabs:

1. **Device Tab**:
   - Displays device node path (`/dev/sda`), custom volume name, and installed firmware version.
   - Drive bay visualization with text info on the left and colored rectangular status LEDs on the right (Green, Yellow, Red, Red-Green, Gray).
   - Capacity usage progress bar.

2. **Tools Tab**:
   - **Blink Lights**: Triggers front-panel light test.
   - **Rename Drobo Unit**: Input text field pre-populated with the current unit name; click **Rename** to save.
   - **Dump Diagnostics**: Writes encrypted diagnostic dump to `/tmp/DroboDiag_YYYY_MM_DD_HH_MM_<rand_id>.log`.
   - **Show Diag Log**: Opens a custom directory browser modal to select and view any decoded diagnostic log file in a monospace viewer.
   - **Shutdown / Standby**: Sends power down / standby command.
   - **Refresh Data**: Re-opens device nodes and updates status.

3. **Format Tab**:
   - Choose file system target (**Ext3**, **FAT32**, **NTFS**).
   - Click **Generate Format Script** to create an executable partition and format script at `/tmp/fmtscript`.

4. **Options Tab**:
   - Toggle **Dual Disk Redundancy**.
   - Configure **Spin Down Delay** (enable toggle & minute duration).
   - Configure **Manual Volume Management**.
   - Set **Static IP Address** and **NetMask** (for iSCSI / DroboShare units).
   - Adjust **Yellow Alert %** and **Red Alert %** threshold sliders.
   - Click **Save Options to Device** to persist settings directly to the Drobo hardware.

---

## Technical Errata & Protocols

- **SCSI IOCTL Interface**: SCSI Command Descriptor Blocks (CDBs) are executed via Linux `SG_IO` ioctls using 10-byte CDB structures (`0x5a` for subpage queries, `0x55` for mode select configuration writes, and `0xea` for commands and diagnostics streaming).
- **Host OS Filesystem Fallback**: If the Drobo hardware subpage returns `No Partitions` or `NO FORMAT` (due to non-proprietary Linux partitioning via `parted`/`mkfs`), `droboapi` automatically inspects Linux `/proc/partitions` and `blkid` to report valid host filesystems (`ext3`, `ext4`, `ntfs`, `vfat`).
- **XOR Diagnostic Decryption**: Diagnostic dumps produced by Drobo units are encrypted with a 1-byte rolling XOR key computed from byte 0 (`data[0] ^ b'-'`). `decode_diagnostics` handles decryption automatically.

---

## License

Derived from the original `drobo-utils` codebase by Peter Silva. Licensed under GPL-2.0 / GNU General Public License v2.
