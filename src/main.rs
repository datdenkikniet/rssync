use std::{
    fmt::Write as FmtWrite,
    io::{BufRead, BufReader, BufWriter, Read, Write},
    net::{TcpListener, TcpStream},
    os::unix::prelude::OsStrExt,
    path::PathBuf,
    time::Duration,
};

use bitflags::bitflags;

const VER_MAJOR: u32 = 31;
const VER_MINOR: u32 = 0;
const INTERNAL_BUFFER_SIZE: usize = 2048;
const MAX_LINE_LENGTH: usize = 64;
const VERSION: Version = Version {
    major: VER_MAJOR,
    minor: VER_MINOR,
};

pub struct Rsyncd<'a> {
    buffer: [u8; INTERNAL_BUFFER_SIZE],
    tcp_read: BufReader<&'a TcpStream>,
    tcp_write: &'a TcpStream,
    checksum_seed: u32,
}

#[derive(Debug, Clone, Copy)]
pub struct Version {
    major: u32,
    minor: u32,
}

#[derive(Debug, Clone)]
pub enum RsyncMessage {
    Data { length: usize },
    XferError,
    Info,
    Error,
    Warning,
    SocketError,
    Utf8Error,
    Log,
    Client,
    Redo,
    Stats,
    IoError,
    IoTimeout,
    Noop,
    ErrorExit,
    Success,
    Deleted,
    NoSend,
    Invalid,
}

#[derive(Debug)]
pub enum VersionParseError {
    InvalidVersionNumber,
    IncompleteVersionSpecifier,
}

impl Version {
    pub fn parse(data: &str) -> Result<Self, VersionParseError> {
        let mut split = data.split(".");
        let (ver_maj, ver_min) = (split.next(), split.next());

        let (ver_maj, ver_min) = match (ver_maj, ver_min) {
            (Some(vmaj), Some(vmin)) => {
                println!("{}, {}", vmaj, vmin);
                match (u32::from_str_radix(vmaj, 10), u32::from_str_radix(vmin, 10)) {
                    (Ok(vmar), Ok(vmin)) => (vmar, vmin),
                    _ => return Err(VersionParseError::InvalidVersionNumber),
                }
            }
            _ => return Err(VersionParseError::IncompleteVersionSpecifier),
        };

        Ok(Version {
            major: ver_maj,
            minor: ver_min,
        })
    }

    pub fn serialize(&self, string: &mut String) {
        write!(string, "{}.{}", self.major, self.minor).ok();
    }
}

#[derive(Debug, Clone)]
pub enum Query {
    ModuleList,
    FileList { module: PathBuf },
}

#[derive(Debug, Clone)]
pub enum QueryParseError {
    InvalidQuery,
}

impl Query {
    pub fn parse(data: &str) -> Result<Self, QueryParseError> {
        if data == "" || data == "#list" {
            Ok(Self::ModuleList)
        } else {
            Ok(Self::FileList {
                module: data.into(),
            })
        }
    }
}

impl Default for Version {
    fn default() -> Self {
        Self {
            major: 31,
            minor: 0,
        }
    }
}

#[derive(Debug)]
pub enum ClientError {
    InvalidHeader,
    VersionParseError(VersionParseError),
    InvalidQuery(QueryParseError),
    InvalidInput,
}

bitflags! {
  pub struct FileMode: u32 {

  }
}

bitflags! {
  pub struct FileFlags: u32 {
    const FLAG_TOP_DIR = (1<<0);
    const FLAG_OWNED_BY_US= (1<<0);
    const FLAG_FILE_SENT =(1<<1);
    const FLAG_DIR_CREATED= (1<<1);
    const FLAG_CONTENT_DIR= (1<<2);
    const FLAG_MOUNT_DIR =(1<<3);
    const FLAG_SKIP_HLINK= (1<<3);
    const FLAG_DUPLICATE =(1<<4);
    const FLAG_MISSING_DIR =(1<<4);
    const FLAG_HLINKED =(1<<5);
    const FLAG_HLINK_FIRST= (1<<6);
    const FLAG_IMPLIED_DIR= (1<<6);
    const FLAG_HLINK_LAST =(1<<7);
    const FLAG_HLINK_DONE =(1<<8);
    const FLAG_LENGTH64 =(1<<9);
    const FLAG_SKIP_GROUP= (1<<10);
    const FLAG_TIME_FAILED =(1<<11);
    const FLAG_MOD_NSEC =(1<<12);
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

pub struct FileList<'a> {
    files: &'a Vec<File>,
}

#[derive(Clone, Debug)]
pub struct File {
    dirname: PathBuf,
    basename: PathBuf,
    modtime: u32,
    filelen: u64,
    mode: u32,
    flags: FileFlags,
    name_type: NameType,
}

#[derive(Debug)]
pub enum RsyncdError {
    Io(std::io::Error),
    ClientError(ClientError),
    LineTooLarge,
    Unsupported(&'static str),
}

impl From<std::io::Error> for RsyncdError {
    fn from(io: std::io::Error) -> Self {
        Self::Io(io)
    }
}

impl<'a> Rsyncd<'a> {
    fn read_line(&mut self, delimiter: u8) -> Result<String, RsyncdError> {
        let mut data = Vec::with_capacity(256);
        self.tcp_read.read_until(delimiter, &mut data)?;

        let data_len = data.len() - 1;
        match std::str::from_utf8(&data[..data_len]) {
            Ok(string) => Ok(string.to_string()),
            Err(_) => Err(RsyncdError::ClientError(ClientError::InvalidInput)),
        }
    }

    fn read_init(&mut self) -> Result<Version, RsyncdError> {
        let result = self.read_line(0x0A)?;

        if result.starts_with("@RSYNCD: ") {
            let (_, version) = result.split_at(9);
            match Version::parse(version) {
                Ok(version) => Ok(version),
                Err(err) => Err(RsyncdError::ClientError(ClientError::VersionParseError(
                    err,
                ))),
            }
        } else {
            Err(RsyncdError::ClientError(ClientError::InvalidHeader))
        }
    }

    fn send_init(&mut self) -> Result<(), RsyncdError> {
        let mut value = String::with_capacity(64);

        value.push_str("@RSYNCD: ");
        VERSION.serialize(&mut value);
        value.push_str("\n");
        self.tcp_write.write(value.as_bytes())?;

        Ok(())
    }

    fn send_file(&mut self, file: &File) -> Result<(), RsyncdError> {
        Ok(())
    }

    fn read_query(&mut self) -> Result<Query, RsyncdError> {
        let result = self.read_line(0x0A)?;
        Query::parse(&result)
            .map_err(|err| RsyncdError::ClientError(ClientError::InvalidQuery(err)))
    }

    fn server_send_ok(&mut self) -> Result<(), RsyncdError> {
        self.tcp_write
            .write("@RSYNCD: OK\n".as_bytes())
            .map_err(|err| RsyncdError::Io(err))
            .map(|_| ())
    }

    fn server_read_args(&mut self) -> Result<Vec<String>, RsyncdError> {
        let mut arguments = Vec::new();
        loop {
            let string = self.read_line(0x00)?;
            if string.len() == 0 {
                break;
            } else {
                arguments.push(string);
            }
        }
        Ok(arguments)
    }

    fn msg_from_header(data: &[u8]) -> (u8, u32) {
        let data_bytes: u32 = (data[2] as u32) << 16 | (data[1] as u32) << 8 | data[0] as u32;
        let tag = data[3].wrapping_sub(0x07);

        (tag, data_bytes)
    }

    fn msg_to_header(message: RsyncMessage) -> [u8; 4] {
        let mut data = [0u8; 4];
        match message {
            RsyncMessage::Data { length } => {
                let tag = 0x07;
                data[3] = tag;
                data[2] = ((length >> 16) as u8) & 0xFF;
                data[1] = ((length >> 8) as u8) & 0xFF;
                data[0] = (length as u8) & 0xFF;
            }
            _ => {}
        }
        data
    }

    fn read_message(&mut self) -> Result<RsyncMessage, RsyncdError> {
        let mut data = [0u8; 4];
        self.tcp_read.read_exact(&mut data)?;
        let (tag, data) = Self::msg_from_header(&data);

        match tag {
            0x00 => Ok(RsyncMessage::Data {
                length: data as usize,
            }),
            _ => Err(RsyncdError::Unsupported(
                "This message type is not supported",
            )),
        }
    }

    fn send_module_list(&mut self) -> Result<(), RsyncdError> {
        let mut value = String::with_capacity(128);
        value.push_str("module_one\nmodule_two\nmodule_three\n");
        self.tcp_write.write(value.as_bytes())?;
        Ok(())
    }

    fn write_varlong(value: u64, min_size: usize, buffer: &mut Vec<u8>) {
        let mut bytes = [0u8; 9];
        let mut byte_count = 8;

        let mut offset = 1;
        value.to_le_bytes().iter().for_each(|byte| {
            bytes[offset] = *byte;
            offset += 1;
        });

        while byte_count > min_size && bytes[byte_count] == 0 {
            byte_count -= 1;
        }

        let bit_magic = 1 << (7 - byte_count + min_size);
        let last_byte = bytes[byte_count];
        if last_byte >= bit_magic {
            byte_count += 1;
            bytes[0] = !(bit_magic - 1);
        } else if byte_count > min_size {
            bytes[0] = bytes[byte_count] | !((bit_magic << 1) - 1)
        } else {
            bytes[0] = bytes[byte_count];
        }

        buffer.write(&bytes[..byte_count]).ok();
    }

    fn send_file_list(&mut self, file_list: &Vec<File>) -> Result<(), RsyncdError> {
        let mut data = Vec::with_capacity(128);
        for file in file_list.iter() {
            // Send xfer flags
            // Push extended flags
            data.push(1 << 2);
            data.push(0x00);

            // File name length and file name
            // l1 (only if bit 6 is set in xfer flags)
            // data.push(<characters to take from previous name>)
            // l2
            data.push(file.basename.as_os_str().len() as u8);
            // File name
            data.write(file.basename.as_os_str().as_bytes())?;

            // File length
            Self::write_varlong(file.filelen, 3, &mut data);

            // Mod time
            data.write(&file.modtime.to_le_bytes())?;

            // File(?) mode
            data.write(&file.mode.to_le_bytes())?;
        }

        // Signal end of list
        data.push(0);

        let header_data = Self::msg_to_header(RsyncMessage::Data { length: data.len() });

        self.tcp_write.write(&header_data)?;
        self.tcp_write.write(&data)?;

        println!("{:X?}", data);

        Ok(())
    }

    fn send_exit(&mut self) -> Result<(), RsyncdError> {
        let mut value = String::with_capacity(128);
        value.push_str("@RSYNCD: EXIT\n");
        self.tcp_write.write(value.as_bytes())?;
        Ok(())
    }

    fn send_compat_flags(&mut self) -> Result<(), RsyncdError> {
        self.tcp_write
            .write(&[0x00])
            .map(|_| ())
            .map_err(|e| e.into())
    }

    fn send_checksum_seed(&mut self) -> Result<(), RsyncdError> {
        self.tcp_write
            .write(&self.checksum_seed.to_le_bytes())
            .map(|_| ())
            .map_err(|e| e.into())
    }
}

fn main() -> Result<(), RsyncdError> {
    let socket = TcpListener::bind("127.0.0.1:8000").unwrap();

    loop {
        let incoming = socket.incoming().next().unwrap().unwrap();

        let mut rsyncd = Rsyncd {
            buffer: [0u8; 2048],
            tcp_read: BufReader::new(&incoming),
            tcp_write: &incoming,
            checksum_seed: 1,
        };
        rsyncd.send_init().ok();
        println!("Read init: {:?}", rsyncd.read_init());
        println!("Read query: {:?}", rsyncd.read_query());
        println!("Send OK: {:?}", rsyncd.server_send_ok());
        println!("Read args: {:?}", rsyncd.server_read_args());
        println!("Send compat flags: {:?}", rsyncd.send_compat_flags());
        println!("Send checksum seed: {:?}", rsyncd.send_checksum_seed());
        let message = rsyncd.read_message()?;
        println!("Receive msg: {:?}", message);
        match message {
            RsyncMessage::Data { length } => {
                let mut data = Vec::with_capacity(length as usize);
                for _ in 0..length {
                    data.push(0);
                }
                rsyncd.tcp_read.read_exact(&mut data)?;
                println!("Received data: {:X?}", data);
            }
            _ => {}
        }

        println!(
            "Send file list: {:?}",
            rsyncd.send_file_list(&vec![
                File {
                    dirname: PathBuf::from("dot"),
                    basename: PathBuf::from("."),
                    modtime: 5,
                    filelen: 420,
                    mode: 0x1ED,
                    flags: FileFlags::FLAG_TOP_DIR,
                    name_type: NameType::DotDir,
                },
                File {
                    dirname: PathBuf::from("dot"),
                    basename: PathBuf::from("base_name"),
                    modtime: 5,
                    filelen: 0xDEADBEEF,
                    mode: 0x1ED,
                    flags: FileFlags::FLAG_TOP_DIR,
                    name_type: NameType::Normal,
                }
            ])
        );
        std::thread::sleep(Duration::from_secs(5));

        // println!("Send files: {:?}", rsyncd.send_files()?);
        // println!("Send exit: {:?}", rsyncd.send_exit()?);
    }
}
