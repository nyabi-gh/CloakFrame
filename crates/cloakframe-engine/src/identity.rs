use std::io;
use std::path::Path;

/// Which file a path names, independent of how the path is spelled (case, `..`, links to the
/// same file). Two paths with the same identity are the same file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FileIdentity {
    device: u64,
    file: u128,
}

/// Follows symbolic links, like `std::fs::metadata`.
#[cfg(unix)]
pub fn identity(path: &Path) -> io::Result<FileIdentity> {
    use std::os::unix::fs::MetadataExt;
    let metadata = std::fs::metadata(path)?;
    Ok(FileIdentity {
        device: metadata.dev(),
        file: u128::from(metadata.ino()),
    })
}

/// Follows symbolic links and junctions, like `std::fs::metadata`.
#[cfg(windows)]
#[allow(unsafe_code)]
pub fn identity(path: &Path) -> io::Result<FileIdentity> {
    use std::os::windows::fs::OpenOptionsExt;
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_FLAG_BACKUP_SEMANTICS, FILE_ID_INFO, FileIdInfo, GetFileInformationByHandleEx,
    };

    // Directories can only be opened with backup semantics; no access right is needed to read
    // the ID.
    let file = std::fs::OpenOptions::new()
        .access_mode(0)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
        .open(path)?;
    let mut info = FILE_ID_INFO {
        VolumeSerialNumber: 0,
        FileId: windows_sys::Win32::Storage::FileSystem::FILE_ID_128 {
            Identifier: [0; 16],
        },
    };
    // SAFETY: the handle is open for the duration of the call and `info` is a correctly sized
    // FILE_ID_INFO that the call fills in.
    let ok = unsafe {
        GetFileInformationByHandleEx(
            file.as_raw_handle(),
            FileIdInfo,
            (&raw mut info).cast(),
            size_of::<FILE_ID_INFO>() as u32,
        )
    };
    if ok == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(FileIdentity {
        device: info.VolumeSerialNumber,
        file: u128::from_le_bytes(info.FileId.Identifier),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_file_through_different_paths_has_one_identity() {
        let dir = crate::testdir::TestDir::new();
        let file = dir.write("a/one.jpg");
        let other = dir.write("a/two.jpg");
        let via_dotdot = dir.path().join("a/../a/one.jpg");
        assert_eq!(identity(&file).unwrap(), identity(&via_dotdot).unwrap());
        assert_ne!(identity(&file).unwrap(), identity(&other).unwrap());
        assert_ne!(
            identity(&file).unwrap(),
            identity(&dir.path().join("a")).unwrap()
        );
        assert_eq!(
            identity(&dir.path().join("missing")).unwrap_err().kind(),
            io::ErrorKind::NotFound
        );
    }

    #[test]
    fn hard_links_share_an_identity() {
        let dir = crate::testdir::TestDir::new();
        let file = dir.write("photo.jpg");
        let link = dir.path().join("linked.jpg");
        std::fs::hard_link(&file, &link).unwrap();
        assert_eq!(identity(&file).unwrap(), identity(&link).unwrap());
    }
}
