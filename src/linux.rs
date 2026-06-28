use std::ffi::CString;
use std::io::{Error, ErrorKind, Result};
use std::os::unix::prelude::OsStrExt;
use std::path::Path;

unsafe fn renameat2(
    olddirfd: libc::c_int,
    oldpath: *const libc::c_char,
    newdirfd: libc::c_int,
    newpath: *const libc::c_char,
    flags: libc::c_uint,
) -> libc::c_int {
    unsafe {
        libc::syscall(
            libc::SYS_renameat2 as libc::c_long,
            olddirfd,
            oldpath,
            newdirfd,
            newpath,
            flags,
        ) as libc::c_int
    }
}

pub fn rename_exclusive(from: &Path, to: &Path) -> Result<()> {
    let from_str = CString::new(from.as_os_str().as_bytes())?;
    let to_str = CString::new(to.as_os_str().as_bytes())?;
    let ret = unsafe {
        renameat2(
            libc::AT_FDCWD,
            from_str.as_ptr(),
            libc::AT_FDCWD,
            to_str.as_ptr(),
            libc::RENAME_NOREPLACE,
        )
    };

    if ret == -1 {
        let error = Error::last_os_error();
        // EINVAL is returned if `flags` is invalid or the file system doesn't
        // support the operation. ENOSYS is returned if the running kernel
        // doesn't implement renameat2.
        if matches!(error.raw_os_error(), Some(libc::EINVAL | libc::ENOSYS)) {
            Err(Error::from(ErrorKind::Unsupported))
        } else {
            Err(error)
        }
    } else {
        Ok(())
    }
}

#[derive(PartialEq, Eq, PartialOrd, Ord)]
struct Version(u64);

impl Version {
    const fn new(major: u16, minor: u16, patch: u16) -> Self {
        Self(((major as u64) << 32) | ((minor as u64) << 16) | patch as u64)
    }
}

fn get_kernel_version() -> Result<Version> {
    let version = std::fs::read_to_string("/proc/version")?;
    let version_bytes = version.as_bytes();

    let major_begin = version_bytes
        .iter()
        .position(|c| c.is_ascii_digit())
        .ok_or(ErrorKind::InvalidData)?;
    let major_end = major_begin
        + version_bytes[major_begin..]
            .iter()
            .position(|c| *c == b'.')
            .ok_or(ErrorKind::InvalidData)?;

    if major_end == version_bytes.len() - 1 {
        return Err(ErrorKind::InvalidData.into());
    }

    let minor_begin = major_end + 1;
    let minor_end = minor_begin
        + version_bytes[minor_begin..]
            .iter()
            .position(|c| *c == b'.')
            .ok_or(ErrorKind::InvalidData)?;

    if minor_end == version_bytes.len() - 1 {
        return Err(ErrorKind::InvalidData.into());
    }

    let patch_begin = minor_end + 1;
    let patch_end = patch_begin
        + version_bytes[patch_begin..]
            .iter()
            .position(|c| !c.is_ascii_digit())
            .ok_or(ErrorKind::InvalidData)?;

    let major = version[major_begin..major_end]
        .parse()
        .map_err(|_| ErrorKind::InvalidData)?;
    let minor = version[minor_begin..minor_end]
        .parse()
        .map_err(|_| ErrorKind::InvalidData)?;
    let patch = version[patch_begin..patch_end]
        .parse()
        .map_err(|_| ErrorKind::InvalidData)?;

    Ok(Version::new(major, minor, patch))
}

fn get_filesystem_type(path: &Path) -> Result<u64> {
    let path_str = CString::new(path.as_os_str().as_bytes())?;
    let mut buf = std::mem::MaybeUninit::<libc::statfs>::uninit();
    let ret = unsafe { libc::statfs(path_str.as_ptr(), buf.as_mut_ptr()) };

    if ret == -1 {
        return Err(Error::last_os_error());
    }

    Ok(unsafe { buf.assume_init() }.f_type as u64)
}

const FS_EXT4: u64 = libc::EXT4_SUPER_MAGIC as u64;
const FS_BTRFS: [u64; 2] = [
    libc::BTRFS_SUPER_MAGIC as u64,
    0x73727279, // BTRFS_TEST_MAGIC
];
const FS_TMPFS: u64 = libc::TMPFS_MAGIC as u64;
const FS_CIFS: u64 = 0xff534d42; // CIFS_MAGIC_NUMBER
const FS_XFS: u64 = 0x58465342; // XFS_SUPER_MAGIC

// EXT2_SUPER_MAGIC is the same as EXT4_SUPER_MAGIC.
const FS_EXT2: u64 = 0xef51; // EXT2_OLD_SUPER_MAGIC
const FS_MINIX: [u64; 5] = [
    libc::MINIX_SUPER_MAGIC as u64,
    libc::MINIX_SUPER_MAGIC2 as u64,
    libc::MINIX2_SUPER_MAGIC as u64,
    libc::MINIX2_SUPER_MAGIC2 as u64,
    libc::MINIX3_SUPER_MAGIC as u64,
];
const FS_REISERFS: u64 = libc::REISERFS_SUPER_MAGIC as u64;
const FS_JFS: u64 = 0x3153464a; // JFS_SUPER_MAGIC
                                // vfat was discovered experimentally. It doesn't appear in the man page or the
                                // magic.h header.
const FS_VFAT: u64 = 0x7c7c6673;
const FS_BPF: u64 = libc::BPF_FS_MAGIC as u64;

pub fn rename_exclusive_is_atomic(path: &Path) -> Result<bool> {
    let kernel = get_kernel_version()?;
    let fs = get_filesystem_type(path)?;

    // The man page for renameat2 says this:
    //
    //  - ext4 (Linux 3.15);
    //  - btrfs, tmpfs, and cifs (Linux 3.17);
    //  - xfs (Linux 4.0);
    //  - Support for many other filesystems was added in Linux 4.9, including
    //    ext2, minix, reiserfs, jfs, vfat, and bpf.

    if kernel >= Version::new(3, 15, 0) {
        if fs == FS_EXT4 {
            return Ok(true);
        }
    }

    if kernel >= Version::new(3, 17, 0) {
        if FS_BTRFS.contains(&fs) || [FS_TMPFS, FS_CIFS].contains(&fs) {
            return Ok(true);
        }
    }

    if kernel >= Version::new(4, 0, 0) {
        if fs == FS_XFS {
            return Ok(true);
        }
    }

    if kernel >= Version::new(4, 9, 0) {
        // The man page says "including" which implies that this is not an
        // exhaustive list.
        if [FS_EXT2, FS_REISERFS, FS_JFS, FS_VFAT, FS_BPF].contains(&fs) || FS_MINIX.contains(&fs) {
            return Ok(true);
        }
    }

    Ok(false)
}
