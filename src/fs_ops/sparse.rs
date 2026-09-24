#[cfg(target_os = "linux")]
use std::fs;
#[cfg(target_os = "linux")]
use std::io::{self, Read, Seek, SeekFrom, Write};
#[cfg(target_os = "linux")]
use std::os::unix::io::AsRawFd;
#[cfg(target_os = "linux")]
use std::path::Path;

/// Copies file content from src to dst while preserving holes, so sparse files
/// stay sparse even across devices. Only data ranges (found via
/// lseek(SEEK_DATA/SEEK_HOLE)) are written; the destination is then extended
/// to the full apparent size so a trailing hole survives.
#[cfg(target_os = "linux")]
pub(crate) fn copy_sparse(src: &Path, dst: &Path) -> io::Result<()> {
    let mut src_file = fs::File::open(src)?;
    let mut dst_file = fs::File::create(dst)?;

    const BUF_SIZE: i64 = 1024 * 1024; // 1 MiB
    let mut buf = vec![0u8; BUF_SIZE as usize];

    let mut offset: i64 = 0;
    loop {
        let data_start = unsafe { libc::lseek(src_file.as_raw_fd(), offset, libc::SEEK_DATA) };
        if data_start < 0 {
            match io::Error::last_os_error().raw_os_error() {
                Some(libc::ENXIO) => break, // no more data; the rest is a hole
                // Filesystem does not support data/hole seeking: copy plainly.
                Some(libc::EOPNOTSUPP) => {
                    return plain_copy(&mut src_file, &mut dst_file, &mut buf);
                }
                _ => return Err(io::Error::last_os_error()),
            }
        }
        let hole_start = unsafe { libc::lseek(src_file.as_raw_fd(), data_start, libc::SEEK_HOLE) };
        if hole_start < 0 {
            return Err(io::Error::last_os_error());
        }

        // Copy the data range [data_start, hole_start).
        let mut pos = data_start;
        while pos < hole_start {
            let to_read = std::cmp::min(BUF_SIZE, hole_start - pos) as usize;
            let nread = unsafe {
                libc::pread(
                    src_file.as_raw_fd(),
                    buf.as_mut_ptr() as *mut _,
                    to_read,
                    pos,
                )
            };
            if nread <= 0 {
                return Err(io::Error::last_os_error());
            }
            dst_file.seek(SeekFrom::Start(pos as u64))?;
            dst_file.write_all(&buf[..nread as usize])?;
            pos += nread as i64;
        }
        offset = hole_start;
    }

    // Extend dst to the full apparent size so a trailing hole is preserved.
    let len = src_file.metadata()?.len();
    dst_file.set_len(len)?;
    Ok(())
}

/// Plain chunked copy of an already-open file pair (used when the filesystem
/// does not support SEEK_DATA/SEEK_HOLE). Rewrites from offset 0, so it is
/// safe even if sparse ranges were written before falling back.
#[cfg(target_os = "linux")]
pub(crate) fn plain_copy(
    src_file: &mut fs::File,
    dst_file: &mut fs::File,
    buf: &mut [u8],
) -> io::Result<()> {
    src_file.seek(SeekFrom::Start(0))?;
    dst_file.set_len(0)?; // clean slate in case sparse ranges were written
    loop {
        let nread = src_file.read(buf)?;
        if nread == 0 {
            break;
        }
        dst_file.write_all(&buf[..nread])?;
    }
    Ok(())
}
