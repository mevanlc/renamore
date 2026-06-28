use std::ffi::{CStr, CString};
use std::io::{Error, ErrorKind, Result};
use std::os::unix::prelude::OsStrExt;
use std::path::Path;

fn unsupported_or(error: Error) -> Error {
    if error.kind() == ErrorKind::Unsupported {
        return Error::from(ErrorKind::Unsupported);
    }

    if let Some(code) = error.raw_os_error() {
        if code == libc::ENOTSUP || code == libc::EOPNOTSUPP {
            return Error::from(ErrorKind::Unsupported);
        }
    }

    error
}

pub fn rename_exclusive(from: &Path, to: &Path) -> Result<()> {
    let from_str = CString::new(from.as_os_str().as_bytes())?;
    let to_str = CString::new(to.as_os_str().as_bytes())?;
    let ret = unsafe { libc::renamex_np(from_str.as_ptr(), to_str.as_ptr(), libc::RENAME_EXCL) };

    if ret == -1 {
        Err(unsupported_or(Error::last_os_error()))
    } else {
        Ok(())
    }
}

#[repr(C)]
struct AttributeBuf {
    length: u32,
    volume: libc::vol_capabilities_attr_t,
}

fn get_volume_root(path: &Path) -> Result<CString> {
    let path_str = CString::new(path.as_os_str().as_bytes())?;
    let mut buf = std::mem::MaybeUninit::<libc::statfs>::uninit();
    let ret = unsafe { libc::statfs(path_str.as_ptr(), buf.as_mut_ptr()) };

    if ret == -1 {
        return Err(Error::last_os_error());
    }

    let stat = unsafe { buf.assume_init() };
    Ok(unsafe { CStr::from_ptr(stat.f_mntonname.as_ptr()) }.to_owned())
}

pub fn rename_exclusive_is_atomic(path: &Path) -> Result<bool> {
    let root = get_volume_root(path)?;
    let mut list = libc::attrlist {
        bitmapcount: libc::ATTR_BIT_MAP_COUNT,
        reserved: 0,
        commonattr: 0,
        volattr: libc::ATTR_VOL_INFO | libc::ATTR_VOL_CAPABILITIES,
        dirattr: 0,
        fileattr: 0,
        forkattr: 0,
    };
    let mut buf = AttributeBuf {
        length: 0,
        volume: libc::vol_capabilities_attr_t {
            capabilities: [0; 4],
            valid: [0; 4],
        },
    };

    let ret = unsafe {
        libc::getattrlist(
            root.as_ptr(),
            std::ptr::addr_of_mut!(list).cast(),
            std::ptr::addr_of_mut!(buf).cast(),
            std::mem::size_of::<AttributeBuf>(),
            0,
        )
    };

    if ret == -1 {
        return Err(Error::last_os_error());
    }

    if (buf.length as usize) < std::mem::size_of::<AttributeBuf>() {
        return Ok(false);
    }

    let idx = libc::VOL_CAPABILITIES_INTERFACES;
    let mask = libc::VOL_CAP_INT_RENAME_EXCL;
    let valid = buf.volume.valid[idx];
    let capabilities = buf.volume.capabilities[idx];

    Ok((valid & mask) != 0 && (capabilities & mask) != 0)
}
