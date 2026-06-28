use std::ffi::CString;
use std::io::{Error, ErrorKind, Result};
use std::os::unix::prelude::OsStrExt;
use std::path::Path;

pub fn rename_exclusive(from: &Path, to: &Path) -> Result<()> {
    let from_str = CString::new(from.as_os_str().as_bytes())?;
    let to_str = CString::new(to.as_os_str().as_bytes())?;
    let ret = unsafe { libc::renamex_np(from_str.as_ptr(), to_str.as_ptr(), libc::RENAME_EXCL) };

    if ret == -1 {
        let error = Error::last_os_error();
        // EINVAL is returned if `flags` is invalid.
        // ENOTSUP is returned if the file system doesn't support the operation.
        if error.kind() == ErrorKind::InvalidInput {
            Err(Error::from(ErrorKind::Unsupported))
        } else {
            Err(error)
        }
    } else {
        Ok(())
    }
}

#[repr(C)]
struct AttributeBuf {
    length: u32,
    volume: libc::vol_capabilities_attr_t,
}

pub fn rename_exclusive_is_atomic(path: &Path) -> Result<bool> {
    let path_str = CString::new(path.as_os_str().as_bytes())?;
    let mut list = libc::attrlist {
        bitmapcount: libc::ATTR_BIT_MAP_COUNT,
        reserved: 0,
        commonattr: 0,
        volattr: libc::ATTR_VOL_CAPABILITIES,
        dirattr: 0,
        fileattr: 0,
        forkattr: 0,
    };
    let mut buf = std::mem::MaybeUninit::<AttributeBuf>::uninit();

    let ret = unsafe {
        libc::getattrlist(
            path_str.as_ptr(),
            std::ptr::addr_of_mut!(list).cast(),
            buf.as_mut_ptr().cast(),
            std::mem::size_of::<AttributeBuf>(),
            0,
        )
    };

    if ret == -1 {
        return Err(Error::last_os_error());
    }

    let attrs = unsafe { buf.assume_init_ref() };
    let capabilities = attrs.volume.capabilities[libc::VOL_CAPABILITIES_INTERFACES];

    Ok(capabilities & libc::VOL_CAP_INT_RENAME_EXCL != 0)
}
