//! Raw ioctl interface to /dev/evo8.
//!
//! Mirrors `struct evo_ctrl_xfer` and `EVO_CTRL_TRANSFER` from kmod/evo_raw.c.

use std::os::unix::io::RawFd;

use evo_protocol::controls::{REQ_CUR, REQ_TYPE_GET, REQ_TYPE_SET};

use crate::DriverError;

const EVO_MAX_DATA: usize = 256;

/// Must match `struct evo_ctrl_xfer` in kmod/evo_raw.c exactly (264 bytes, #[repr(C)]).
#[repr(C)]
pub struct EvoCtrlXfer {
    pub b_request_type: u8,
    pub b_request: u8,
    pub w_value: u16,
    pub w_index: u16,
    pub w_length: u16,
    pub data: [u8; EVO_MAX_DATA],
}

const _: () = assert!(
    std::mem::size_of::<EvoCtrlXfer>() == 264,
    "EvoCtrlXfer size must match the C struct (264 bytes)"
);

/// `_IOWR('E', 0, struct evo_ctrl_xfer)`.
/// = (3 << 30) | (264 << 16) | (0x45 << 8) | 0 = 0xC108_4500
pub const EVO_CTRL_TRANSFER: libc::c_ulong = 0xC108_4500;

/// Send a GET_CUR request and return the response bytes.
pub fn get_cur(fd: RawFd, w_value: u16, w_index: u16, length: u16) -> Result<Vec<u8>, DriverError> {
    let mut xfer = EvoCtrlXfer {
        b_request_type: REQ_TYPE_GET,
        b_request: REQ_CUR,
        w_value,
        w_index,
        w_length: length,
        data: [0u8; EVO_MAX_DATA],
    };

    let ret = unsafe { libc::ioctl(fd, EVO_CTRL_TRANSFER, &mut xfer as *mut EvoCtrlXfer) };

    if ret < 0 {
        let errno = unsafe { *libc::__errno_location() };
        if errno == libc::ENODEV {
            return Err(DriverError::Disconnected);
        }
        return Err(DriverError::Transfer(errno));
    }

    let actual = (ret as usize).min(length as usize);
    Ok(xfer.data[..actual].to_vec())
}

/// Send a SET_CUR request.
pub fn set_cur(fd: RawFd, w_value: u16, w_index: u16, data: &[u8]) -> Result<(), DriverError> {
    let mut xfer = EvoCtrlXfer {
        b_request_type: REQ_TYPE_SET,
        b_request: REQ_CUR,
        w_value,
        w_index,
        w_length: data.len() as u16,
        data: [0u8; EVO_MAX_DATA],
    };
    xfer.data[..data.len()].copy_from_slice(data);

    let ret = unsafe { libc::ioctl(fd, EVO_CTRL_TRANSFER, &mut xfer as *mut EvoCtrlXfer) };

    if ret < 0 {
        let errno = unsafe { *libc::__errno_location() };
        if errno == libc::ENODEV {
            return Err(DriverError::Disconnected);
        }
        return Err(DriverError::Transfer(errno));
    }
    Ok(())
}
