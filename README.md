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
 * LUNSIZE setting.
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
| `set <KEY> [VALUE]` | Set unit name, LUN size, sync time, thresholds (`YellowThreshold`, `RedThreshold`), redundancy, spin-down delay, or network options. |
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

## Compatibility Matrix

If you can get your drobo device to be associated with a `/dev/sdX` file (via Linux SCSI generic support), then you will be able to partition, build file systems, and read & write data to it using ordinary system tools.

Drobo-utils / drobo-rutils accesses Drobos via the same files. The software scans those files, asking each device if it is a Drobo. Unfortunately, the way Drobos respond varies, so not all of them respond in a way that the software understands. Even for the same device, different physical interconnects may work with different functionality. There are two levels of access to Drobos: **data** (being able to read and write data) and **full** (meaning that the Drobo responds to the full command and control language).

With that in mind, the compatibility matrix of each device vs. the physical channel is below:

| Model | Interface (Performance MB/s): USB | FW | TCP/IP | iSCSI | eSATA | Maximum LUN Size (ext3) |
| :--- | :--- | :--- | :--- | :--- | :--- | :--- |
| **Drobo (Gen1)** | full (15) | n/a (0) | n/a (0) | n/a (0) | n/a (0) | 2 TiB |
| **Drobo (Gen2)** | full (15) | full? (?) | n/a (0) | n/a (0) | n/a (0) | 2 TiB |
| **Drobo Share** | n/a (0) | n/a (0) | data*1 (15) | n/a (0) | n/a (0) | 2 TiB |
| **Drobo Pro** | full (15) | ? (?) | n/a (0) | full*2 (80) | n/a (0) | 8 TiB |
| **Drobo Elite** | ? (?) | n/a (0) | n/a (0) | ? (?) | n/a (0) | 8 TiB |
| **Drobo S** | full (15) | ? (?) | n/a (0) | n/a (?) | data (0) | 8 TiB |
| **Drobo FS** | n/a (15) | ? (?) | data (0) | n/a (?) | n/a (0) | 2? TiB |

**Legend:**
- **full**: Find/Full: drobo-utils will find the device on its own (auto-discover) and supports full control.
- **data**: Works: device functions for data I/O, but drobo-utils cannot access it for configuration.
- **n/a**: Not applicable (device does not have this interface).
- **\*1**: Droboshare is not usable with drobo-utils directly on a Linux server. If you install Droboshare Augmented Root File System (DARFS) then one can run drobo-utils in line mode on the Droboshare itself.
- **\*2**: Will not detect an iSCSI Drobo out of the box. One must configure the iSCSI subsystem to obtain a `/dev/sdX` file (see [iSCSI Setup](#iscsi-setup) for details). After that point, drobo-utils / drobo-rutils will function properly over iSCSI.

*Note on Generic SCSI:* Linux drivers make disk, CD-ROM, and other peripherals look like SCSI peripherals to applications. Regardless of the physical connection, it is a normal part of the Linux kernel to make the device appear as a "generic SCSI" device.

---

## COFFEE IS HOT

People sued a national fast food chain because their [coffee was hot](http://en.wikipedia.org/wiki/Liebeck_v._McDonald's_Restaurants), but did not have a warning on the cup stating that. For most people, the risk of scalding should be fairly obvious.

Drobo has made it so much easier to obtain much more secure storage, that some with little or no professional storage management experience are getting Drobos. Some have the expectation that Drobos, because they allow for disk failures, replace the need for backups. Sad stories have been told about people putting data only on the Drobo, and nowhere else, and then something happens and they lose the data.

> **PLEASE, PLEASE, PLEASE:** Do not store all your data on a Drobo (or any other single device, from any vendor) with no backups or alternate copies. Eventually, something very bad will happen.

You need to look at your data and determine the backup/data security strategy. If you have never done this, or do not know what it means, please consult the [Deployment](#deployment) section for examples.

---

## LUNSIZE Fits All?

By default, Drobo creates a separate 'disk' visible to the computer for every 2 Terabytes (TiB) of parity-protected capacity on the unit. The natural usage that a Drobo invites in users is to have a single, large device covering all the data on the device. For example, on macOS, users often create 16 TB LUNs on HFS+. This allows all the storage to fit on one large pool. The downside of larger LUNs has to do with startup time and the time to perform a file system check.

Under Linux, unfortunately, with a first generation Drobo, one should limit the volume size to 2 TiB. It is hoped, but not confirmed, that later products support LUNs larger than 2 TiB on Linux. Drobom view / droboguiview therefore limits LUN size to 2 TiB for the moment. The command-line interface can be used to create larger LUNs, but they might not work reliably.

### Notes on LUN Sizes >= 2 TiB:
- On older distributions, there are a couple of gotchas related to Linux tools which aren't 2 TiB ready... to exceed 2 TiB, you need to:
  - Use GPT partitions, which aren't supported by older `fdisk` versions. Tools based on `libparted` work fine, mostly.
  - `gparted` fails on older versions and seems to have a 1 TiB limit on devices.
- On Linux kernel < 2.6.24, the USB layer won't let one address LUNs/offsets > 2 TiB.
- On Linux kernel < 2.6.31, there are reported FireWire problems that prevent devices > 2 TiB from working.
- `ext3` with 4K blocks is supposed to allow file system capacity of 8 TiB. 4K blocks seem to be assigned by default.
- Windows XP does not support LUNs > 2 TiB.

---

## What Kind of File System?

Drobos work well under Linux with `ext3` / `ext4`. One can, of course, set up an NTFS, HFS+, or FAT32 file system if necessary, but it seems actively counter-intuitive on Linux. Developers of drobo-utils have not tested HFS+. Linux does not write to Journaled HFS+ at this point, so HFS+ support is not present. Good success is reported with `ntfs-3g`, but the performance is much lower than what is typically reported with `ext3`. Unless physical movement of the disk between systems is required, the native (`ext3` / `ext4`) format is the best option.

---

## iSCSI Setup

This is a procedure for configuring a Drobo Pro for access via iSCSI (based on community contributions):

1. Connect the Drobo Pro via USB, and manually configure the IP of the device:
   ```bash
   # drobocli options (or via set IP commands)
   # drobocli set IPAddress 192.168.2.80
   # drobocli set NetMask 255.255.255.0
   # drobocli set UseStaticIPAddress True
   ```

2. Disconnect USB, and connect via iSCSI.
3. Install `open-iscsi`:
   ```bash
   sudo apt-get install open-iscsi
   ```
4. Connect the Drobo Pro to the host machine via iSCSI.
5. Query the Drobo Pro's ID:
   ```bash
   sudo iscsiadm --mode discovery --type sendtargets --portal 192.168.2.80
   ```
6. Copy the target ID string returned by `iscsiadm`, something like `iqn.2005-06.com.datarobotics:drobopro.tdb091840080.node0`.
7. Connect to the Drobo Pro:
   ```bash
   sudo iscsiadm --mode node --targetname iqn.2005-06.com.datarobotics:drobopro.tdb091840080.node0 --portal 192.168.2.80:3260 --login
   ```

---

## Drobo Firmware

When firmware update is triggered:
- The status can be verified after upload.
- Execute shutdown to allow lights to flash and unit to go dark.
- Wait 5 seconds, unplug power/USB, re-plug after 10 seconds.
- The unit will boot up with the newly flashed firmware.

---

## SAFETY

Those worried about the safety of using this software should know: it was developed with assistance from the vendor (Data Robotics Inc.), and in every case, based on vendor documentation, and with at least encouragement, if not outright support. For each release, a `QA.rst` file was built, demonstrating the functionality tests run. There are multiple checksum verifications built into the firmware upgrade process, so it is next to impossible to brick a Drobo using the tools. Drobo-utils / drobo-rutils verifies firmware checksums before attempting to upload the image to the device, and the device checks the firmware against the checksums as well. New firmware is loaded into an alternate location from the currently active one, and if activation of the new firmware fails, the Drobo will simply boot the old one.

On the other hand, common sense rules do apply. Setting the LUN size, or re-formatting a Drobo will erase all data whether it is done on Linux or any other operating system. These are power tools, and they can do damage to data if used without proper care. For example, the reliability of any storage unit does not reduce the need for backups—it only makes doing them easier. A Drobo is an excellent place to put backups, but not a substitute for them. Backups are the only way to address "error 18" (the number of inches in front of the keyboard where the source of the issue lies), and no storage unit can protect against fire or flood.

---

## Firmware Compatibility

Drobo has been tested with various historical firmware versions. Any Drobo should be upgradable to modern firmware using the management tools.

For Drobo v1 models:
- **1.0.1**: Very old. DO NOT USE. UPGRADE ASAP.
- **1.0.2**: Works OK in CLI and GUI to view and upgrade firmware. DO NOT USE. UPGRADE ASAP.
- **1.0.3**: GUI and CLI work OK, can upgrade firmware. DO NOT USE. UPGRADE ASAP.
- **1.1.0**: Dashboard works with no issues. From this point, unplugging the USB connector to complete upgrade is not strictly necessary. Firmware prior to here deals poorly with `ext3`.
- **1.1.1 - 1.2.4**: Works without issues (note: 'name' feature not supported by this firmware).
- **1.3.0 / 1.3.5**: Works without issues.

---

## Deployment

No storage unit ever constructed, at any price point, can live up to the expectation of never losing data. There is no magic wand to wave to solve the data security problem. People still need a strategy around backups and their maintenance. Drobo makes it easier to implement a strategy, but does not replace it.

### General Concepts

- **Don't rely on a single device, ever!** Before deploying a storage unit, perform the thought experiment of what will happen if all data on it is lost.
- **Classify Your Data**: Differentiate between easily reproducible data (downloads, build artifacts) and *precious data* (irreplaceable personal photos, tax records, financial accounts). Keep precious data organized and backed up in multiple locations (e.g., local primary storage + offsite or secondary backup Drobo).

### Primary Storage
If a Drobo is used for primary storage for precious data, a second device should be obtained as a backup repository. Keeping all data on a single device without secondary backups is inviting data loss.

### Scratch Storage
Data that can be regenerated or re-downloaded (build object files, TV recordings, package caches) is suitable for scratch storage. Loss of scratch storage is inconvenient, but not catastrophic.

### Backup Repository
A rational and highly recommended configuration for a Drobo is as a backup repository. The Drobo is the destination for automated backups from desktops and laptops.

---

## Troubleshooting

### No Drobos Discovered
If a connected Drobo is not auto-detected:
1. Run the CLI tool with verbose output enabled to see what vendor string the SCSI query returns:
   ```bash
   drobocli status
   ```
2. If the vendor string is non-standard (e.g. `Drobo`), use the explicit `--device /dev/sdX` flag to point directly to the drive node.

### Only One LUN Shows Up?
The default LUN size on Drobos is 2 TiB. If more disk space than the configured LUN size is installed, Drobo will expose a second (or third) LUN as additional `/dev/sdX` nodes (e.g., `/dev/sde`, `/dev/sdf`).

If multiple LUNs should be visible but are missing:
- Ensure the `scsi_mod` kernel module parameter `max_luns` is greater than 1:
  ```bash
  cat /sys/module/scsi_mod/parameters/max_luns
  ```

### Getting Help from DRI
Historical note: Data Robotics Inc. (DRI) provided customer support channels via phone, email, and web ticket systems. Community support was hosted on the `drobo-talk` Google Group.

---

## FAQ

### What LUNSIZE should I use?
2 Terabytes is the safest choice for older devices. Refer to the Compatibility Matrix for unit-specific limits.

### What Happens When I Use A Bigger LUNSIZE?
On unsupported older kernels or firmware, space reclamation upon file deletion may fail, leading to inconsistent LED capacity indicators or inability to access data beyond 2 TiB.

### How Do I Check That a LUNSIZE Works?
1. Create a file system as normal.
2. Fill the physical storage until high capacity indicators illuminate.
3. Delete a large portion of data.
4. Verify that capacity indicators update and drop back down accordingly.

### Can I use ReiserFS, XFS, BTRFS, etc.?
Short answer: No. Drobo's thin-provisioning firmware requires understanding the underlying file system block allocation map. Officially supported formats are `ext3`/`ext4`, `FAT32`, `NTFS`, and `HFS+`.

### If my Drobo breaks, Can I Get My Data back?
The layout of data across drive sets in a Drobo is proprietary. Drives cannot be individually mounted on standard Linux servers or reconstructed with standard `mdadm`/`lvm` tools. You must restore from backup or insert the drive pack into a compatible Drobo chassis.

### My USB Drobo always comes up as a different Disk!
Linux dynamic device naming means USB drives may be assigned `/dev/sdb` or `/dev/sdC` across reboots. Use partition UUIDs in `/etc/fstab` for stable mounting:
```bash
ls -l /dev/disk/by-uuid
```
Example `/etc/fstab` entry:
```
UUID=3cd5d9cc-c227-4ed8-bab2-60c2d71f6e9d  /drobo01  ext3  defaults  0  2
```

### How come the command to build a file system builds an ext2 file system?
An `ext3` file system is an `ext2` file system with an added journal (`mke2fs -j`).

### Can you have different LUNs with different file systems on them?
Yes, multiple partitions per LUN and different supported file systems across LUNs are supported.

### Does Drobo work with LVM?
Not recommended. Standard Linux LVM virtual volume management bypasses Drobo's thin provisioning layer and can confuse firmware space tracking.

### Does Drobo Support Full Disk Encryption?
Full disk encryption (LUKS, TrueCrypt) encrypts raw blocks, making the Drobo firmware perceive all blocks as permanently occupied. Use file-level or filesystem-level encryption (e.g., `EncFS`, `ecryptfs`, or NTFS directory encryption) instead.

### Does Drobo-utils work with USB3 / eSATA?
Older models encapsulated SCSI packets over USB. Newer protocols switched to SATA emulation over USB/eSATA.

### Is it Still Supported?
`drobo-utils` was originally maintained by Peter Silva. `drobo-rutils` is a modern Rust rewrite providing full compatibility for legacy Drobo units on modern Linux kernels.

### I have read everything. Help?
Consult community forums or review hardware diagnostics via `drobocli diags`.

### What is Drobo-utils' License?
GNU General Public License v2 (GPL-2.0).

---

## Credits

The rust version is entirely written by Gemini AI, guided by Peter Silva.

Original `drobo-utils` contributors to the python implementation:
- **Peter Silva**: Primary author and maintainer.
- **Chris Atlee**: Debian packaging.
- **Brad Guillory**: Diagnostics & patches.
- **Joe Krahn**: Design feedback.
- **Andy Grover**: Cleanups.
- **Sebastian Sobolewski**: DroboPro patches and testing.
- **Testers**: robj, Sebastian (Tom Green), ElliotA, Andrew Chalaturnyk.

---

## Administrivia

- Copyright (C) 2026 Peter Silva (Peter.A.Silva@gmail.com)
- Licensed under GPL-2.0. See `LICENSE` for details.

---

## License

Licensed under GPL-2.0 / GNU General Public License v2.
