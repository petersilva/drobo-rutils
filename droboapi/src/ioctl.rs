use crate::error::{DroboError, Result};
use std::fs::{File, OpenOptions};
use std::os::unix::io::AsRawFd;
use std::path::Path;

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct SgIoHdr {
    pub interface_id: i32,
    pub dxfer_direction: i32,
    pub cmd_len: u8,
    pub mx_sb_len: u8,
    pub iovec_count: u16,
    pub dxfer_len: i32,
    pub dxferp: *mut libc::c_void,
    pub cmdp: *const u8,
    pub sbp: *mut u8,
    pub timeout: u32,
    pub flags: u32,
    pub pack_id: i32,
    pub usr_ptr: *mut libc::c_void,
    pub status: u8,
    pub masked_status: u8,
    pub msg_status: u8,
    pub sb_len_wr: u8,
    pub host_status: u16,
    pub driver_status: u16,
    pub resid: i32,
    pub duration: u32,
    pub info: u32,
}

impl Default for SgIoHdr {
    fn default() -> Self {
        Self {
            interface_id: b'S' as i32,
            dxfer_direction: 0,
            cmd_len: 0,
            mx_sb_len: 0,
            iovec_count: 0,
            dxfer_len: 0,
            dxferp: std::ptr::null_mut(),
            cmdp: std::ptr::null(),
            sbp: std::ptr::null_mut(),
            timeout: 4000,
            flags: 0,
            pack_id: 0,
            usr_ptr: std::ptr::null_mut(),
            status: 0,
            masked_status: 0,
            msg_status: 0,
            sb_len_wr: 0,
            host_status: 0,
            driver_status: 0,
            resid: 0,
            duration: 0,
            info: 0,
        }
    }
}

pub const SG_DXFER_TO_DEV: i32 = -2;
pub const SG_DXFER_FROM_DEV: i32 = -3;
pub const SG_IO: u64 = 0x2285;
pub const SG_GET_VERSION_NUM: u64 = 0x2282;

pub const SCSI_IOCTL_GET_IDLUN: u64 = 0x5382;

pub struct DroboIOctl {
    file: File,
    pub path: String,
}

impl DroboIOctl {
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self> {
        let p_str = path.as_ref().to_string_lossy().to_string();
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(path)?;

        Ok(Self { file, path: p_str })
    }

    pub fn get_version(&self) -> Result<i32> {
        let mut version: i32 = 0;
        let res = unsafe {
            libc::ioctl(
                self.file.as_raw_fd(),
                SG_GET_VERSION_NUM as _,
                &mut version as *mut i32,
            )
        };

        if res < 0 {
            Err(DroboError::Io(std::io::Error::last_os_error()))
        } else {
            Ok(version)
        }
    }

    pub fn get_sub_page(&self, sz: usize, mcb: &[u8], out: bool) -> Result<Vec<u8>> {
        let mut io_hdr = SgIoHdr::default();

        if out {
            io_hdr.dxfer_direction = SG_DXFER_TO_DEV;
        } else {
            io_hdr.dxfer_direction = SG_DXFER_FROM_DEV;
        }

        io_hdr.cmd_len = mcb.len() as u8;
        io_hdr.cmdp = mcb.as_ptr();

        let mut sense_buffer = vec![0u8; 64];
        io_hdr.mx_sb_len = sense_buffer.len() as u8;
        io_hdr.sbp = sense_buffer.as_mut_ptr();

        let mut page_buffer = vec![0u8; sz];
        io_hdr.dxfer_len = sz as i32;
        io_hdr.dxferp = page_buffer.as_mut_ptr() as *mut libc::c_void;

        let res = unsafe {
            libc::ioctl(
                self.file.as_raw_fd(),
                SG_IO as _,
                &mut io_hdr as *mut SgIoHdr,
            )
        };

        if res < 0 {
            return Err(DroboError::IoctlError);
        }

        if io_hdr.status != 0 {
            return Err(DroboError::IoctlStatusError(io_hdr.status));
        }

        let retsz = if io_hdr.resid > 0 {
            sz.saturating_sub(io_hdr.resid as usize)
        } else {
            sz
        };

        page_buffer.truncate(retsz);
        Ok(page_buffer)
    }

    pub fn put_sub_page(&self, mcb: &[u8], buffer: &[u8]) -> Result<usize> {
        let mut io_hdr = SgIoHdr::default();

        io_hdr.dxfer_direction = SG_DXFER_TO_DEV;
        io_hdr.cmd_len = mcb.len() as u8;
        io_hdr.cmdp = mcb.as_ptr();

        let mut sense_buffer = vec![0u8; 32];
        io_hdr.mx_sb_len = sense_buffer.len() as u8;
        io_hdr.sbp = sense_buffer.as_mut_ptr();

        let mut data_to_write = buffer.to_vec();
        io_hdr.dxfer_len = data_to_write.len() as i32;
        io_hdr.dxferp = data_to_write.as_mut_ptr() as *mut libc::c_void;

        let res = unsafe {
            libc::ioctl(
                self.file.as_raw_fd(),
                SG_IO as _,
                &mut io_hdr as *mut SgIoHdr,
            )
        };

        if res < 0 {
            return Err(DroboError::IoctlError);
        }

        if io_hdr.status != 0 && io_hdr.status != 2 {
            return Err(DroboError::IoctlStatusError(io_hdr.status));
        }

        let size = if io_hdr.resid > 0 {
            buffer.len().saturating_sub(io_hdr.resid as usize)
        } else {
            buffer.len()
        };

        Ok(size)
    }

    pub fn identify_lun(&self) -> Result<(i32, i32, i32, i32, String)> {
        #[repr(C, packed)]
        struct IdLun {
            channel: u8,
            lun: u8,
            id: u8,
            host: u8,
            host_unique_id: i32,
        }

        let mut idlun = IdLun {
            channel: 0,
            lun: 0,
            id: 0,
            host: 0,
            host_unique_id: 0,
        };

        let res = unsafe {
            libc::ioctl(
                self.file.as_raw_fd(),
                SCSI_IOCTL_GET_IDLUN as _,
                &mut idlun as *mut _ as *mut libc::c_void,
            )
        };

        if res < 0 {
            return Err(DroboError::IoctlError);
        }

        // Standard SCSI Inquiry command (CDB 6-bytes: 0x12, 0, 0, 0, hoholen, 0)
        let hoholen = 36usize;
        let mcb = vec![0x12, 0, 0, 0, hoholen as u8, 0];
        let raw_inq = self.get_sub_page(hoholen, &mcb, false)?;

        let vendor = if raw_inq.len() >= 16 {
            String::from_utf8_lossy(&raw_inq[8..16]).trim().to_string()
        } else if raw_inq.len() >= 8 {
            String::from_utf8_lossy(&raw_inq[0..raw_inq.len()]).trim().to_string()
        } else {
            "Unknown".to_string()
        };

        Ok((
            idlun.host as i32,
            idlun.channel as i32,
            idlun.id as i32,
            idlun.lun as i32,
            vendor,
        ))
    }
}
