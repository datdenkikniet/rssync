use std::{io::Write, os::unix::prelude::OsStrExt, path::PathBuf};

use bitflags::bitflags;
use file_mode::Mode;

use crate::{Rsync, RsyncError};

bitflags! {
  pub struct XferFlags: u32 {
    const TOP_DIR = (1 << 0);
    const SAME_MODE = (1 << 1);
    const EXTENDED_FLAGS = (1 << 2);
    const SAME_UID = (1 << 3);
    const SAME_GID = (1 << 4);
    const SAME_NAME = (1 << 5);
    const LONG_NAME = (1 << 6);
    const SAME_TIME = (1 << 7);
    const SAME_RDEV_MAJOR = (1 << 8);
    const NO_CONTENT_DIR = (1 << 8);
    const HARDLINKED = (1 << 9);
    const USER_NAME_FOLLOWS = (1 << 10);
    const GROUP_NAME_FOLLOWS = (1 << 11);
    const HARDLINK_FIRST = (1 << 12);
    const IO_ERROR_ENDLIST = (1 << 12);
    const MOD_NSEC = (1 << 13);
    const SAME_ATIME = (1 << 14);
    const CRTIME_EQ_MTIME = (1 << 17);
  }
}

impl XferFlags {
    pub fn for_file(file: &File, _previous_file: Option<&File>) -> Self {
        let mut flags = Self::empty();

        if file.get_full_name().as_os_str().len() > 255 {
            todo!();
        }

        if flags.is_empty() && !file.is_directory() {
            flags |= XferFlags::TOP_DIR;
        }

        if flags.bits() > 0xFF {
            flags |= XferFlags::EXTENDED_FLAGS;
        }

        flags
    }

    pub fn write_data_bytes(&self, buffer: &mut Vec<u8>) -> Result<(), RsyncError> {
        let bytes = self.bits().to_le_bytes();
        buffer.push(bytes[0]);
        if self.contains(XferFlags::EXTENDED_FLAGS) {
            buffer.push(bytes[1]);
        }
        Ok(())
    }
}

bitflags! {
  pub struct FileFlags: u32 {
    const FLAG_TOP_DIR = (1 << 0);
    const FLAG_OWNED_BY_US= (1 << 0);
    const FLAG_FILE_SENT =(1 << 1);
    const FLAG_DIR_CREATED= (1 << 1);
    const FLAG_CONTENT_DIR= (1 << 2);
    const FLAG_MOUNT_DIR =(1 << 3);
    const FLAG_SKIP_HLINK= (1 << 3);
    const FLAG_DUPLICATE =(1 << 4);
    const FLAG_MISSING_DIR =(1 << 4);
    const FLAG_HLINKED =(1 << 5);
    const FLAG_HLINK_FIRST= (1 << 6);
    const FLAG_IMPLIED_DIR= (1 << 6);
    const FLAG_HLINK_LAST =(1 << 7);
    const FLAG_HLINK_DONE =(1 << 8);
    const FLAG_LENGTH64 =(1 << 9);
    const FLAG_SKIP_GROUP= (1 << 10);
    const FLAG_TIME_FAILED =(1 << 11);
    const FLAG_MOD_NSEC =(1 << 12);
   }
}

#[derive(Clone, Copy, Debug)]
pub enum NameType {
    Normal,
    SlashEnding,
    DotDir,
    Missing,
}

impl NameType {
    fn try_from_u8(data: u8) -> Option<Self> {
        let name_type = match data {
            0x00 => Self::Normal,
            0x01 => Self::SlashEnding,
            0x02 => Self::DotDir,
            0x03 => Self::Missing,
            _ => return None,
        };
        Some(name_type)
    }

    fn to_u8(self) -> u8 {
        match self {
            NameType::Normal => 0x00,
            NameType::SlashEnding => 0x01,
            NameType::DotDir => 0x02,
            NameType::Missing => 0x03,
        }
    }
}

#[derive(Clone, Debug)]
pub struct File {
    pub dirname: PathBuf,
    pub basename: PathBuf,
    pub modtime: u32,
    pub filelen: u64,
    pub mode: Mode,
    pub flags: FileFlags,
    pub name_type: NameType,
}

impl File {
    pub fn write_data_bytes(&self, buffer: &mut Vec<u8>) -> Result<(), RsyncError> {
        // Send xfer flags
        XferFlags::for_file(self, None).write_data_bytes(buffer)?;

        // File name length and file name
        let name = self.basename.as_os_str();
        // l1 (only if bit 6 is set in xfer flags)
        // data.push(<characters to take from previous name>)
        // l2
        buffer.push(name.len() as u8);
        // File name
        buffer.write(name.as_bytes())?;

        // File length
        Rsync::write_varlong(self.filelen, 3, buffer);

        // Mod time
        buffer.write(&self.modtime.to_le_bytes())?;

        // File(?) mode
        buffer.write(&self.mode.mode().to_le_bytes())?;

        Ok(())
    }

    pub fn get_full_name(&self) -> PathBuf {
        let mut name = self.dirname.clone();
        name.push(self.basename.clone());
        name
    }

    pub fn is_directory(&self) -> bool {
        self.get_full_name().is_dir()
    }
}
