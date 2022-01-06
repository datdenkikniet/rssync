use std::{
    fmt::{Display, Write as FmtWrite},
    io::Write,
    net::{TcpListener, TcpStream},
    path::PathBuf,
};

use file_list::File;
use file_mode::FileType;
use protocol::{ReceiveError, RsyncMessage, RsyncSocket, SendError};

use crate::file_list::{FileFlags, NameType};

mod file_list;
mod protocol;

const VER_MAJOR: u32 = 31;
const VER_MINOR: u32 = 0;

const VERSION: Version = Version {
    major: VER_MAJOR,
    minor: VER_MINOR,
};

pub struct Rsync<'a> {
    socket: RsyncSocket<&'a TcpStream, &'a TcpStream>,
    checksum_seed: u32,
}

#[derive(Debug, Clone, Copy)]
pub struct Version {
    major: u32,
    minor: u32,
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

    pub fn serialize(&self) -> String {
        let mut string = String::new();
        write!(string, "{}.{}", self.major, self.minor).ok();
        string
    }
}

impl Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.serialize())
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

#[derive(Debug)]
pub enum RsyncError {
    Io(std::io::Error),
    ClientError(ClientError),
    LineTooLarge,
    Unsupported(&'static str),
    Receive(ReceiveError),
    Send(SendError),
}

impl From<std::io::Error> for RsyncError {
    fn from(io: std::io::Error) -> Self {
        Self::Io(io)
    }
}

impl From<ReceiveError> for RsyncError {
    fn from(rx: ReceiveError) -> Self {
        Self::Receive(rx)
    }
}

impl From<SendError> for RsyncError {
    fn from(tx: SendError) -> Self {
        Self::Send(tx)
    }
}

impl<'a> Rsync<'a> {
    fn read_line(&mut self, delimiter: u8) -> Result<String, RsyncError> {
        let data = self.socket.read_raw_until(delimiter)?;
        let data_len = data.len() - 1;
        match std::str::from_utf8(&data[..data_len]) {
            Ok(string) => Ok(string.to_string()),
            Err(_) => Err(RsyncError::ClientError(ClientError::InvalidInput)),
        }
    }

    fn do_init(&mut self) -> Result<Version, RsyncError> {
        let value = format!("@RSYNCD: {}\n", VERSION);
        self.socket.send_data(value.as_bytes())?;

        let result = self.read_line(0x0A)?;
        if result.starts_with("@RSYNCD: ") {
            let (_, version) = result.split_at(9);
            match Version::parse(version) {
                Ok(version) => Ok(version),
                Err(err) => Err(RsyncError::ClientError(ClientError::VersionParseError(err))),
            }
        } else {
            Err(RsyncError::ClientError(ClientError::InvalidHeader))
        }
    }

    fn send_file(&mut self, _file: &File) -> Result<(), RsyncError> {
        Ok(())
    }

    fn read_query(&mut self) -> Result<Query, RsyncError> {
        let result = self.read_line(0x0A)?;
        Query::parse(&result).map_err(|err| RsyncError::ClientError(ClientError::InvalidQuery(err)))
    }

    fn server_send_ok(&mut self) -> Result<(), RsyncError> {
        let value = "@RSYNCD: OK\n".as_bytes();
        self.socket.send_data(value)?;
        Ok(())
    }

    fn server_read_args(&mut self) -> Result<Vec<String>, RsyncError> {
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

    fn send_module_list(&mut self) -> Result<(), RsyncError> {
        let value = "module_one\nmodule_two\nmodule_three\n".as_bytes();
        self.socket.send_data(value)?;
        Ok(())
    }

    pub fn write_varlong(value: u64, min_size: usize, buffer: &mut Vec<u8>) {
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

    fn send_file_list(&mut self, file_list: &Vec<File>) -> Result<(), RsyncError> {
        let mut data = Vec::with_capacity(128);
        for file in file_list.iter() {
            file.write_data_bytes(&mut data)?;
        }
        // Signal end of list
        data.push(0);
        self.socket.send_data(data)?;
        Ok(())
    }

    fn send_exit(&mut self) -> Result<(), RsyncError> {
        self.socket.send_data("@RSYNCD: EXIT\n".as_bytes())?;
        Ok(())
    }

    fn send_compat_flags(&mut self) -> Result<(), RsyncError> {
        self.socket.send_data([0x00].as_slice())?;
        Ok(())
    }

    fn send_checksum_seed(&mut self) -> Result<(), RsyncError> {
        self.socket
            .send_data(self.checksum_seed.to_le_bytes().as_slice())?;
        Ok(())
    }
    fn start_multiplexing(&mut self) {
        self.socket.set_multiplexed(true);
    }

    fn read_message(&mut self) -> Result<RsyncMessage<Vec<u8>>, ReceiveError> {
        self.socket.read_message()
    }

    fn receive_sums(&mut self) -> Result<Sums, RsyncError> {
        let chunk_count = self.socket.read_int()?;
        let block_length = self.socket.read_int()?;
        let sum2_length = self.socket.read_int()?;
        let remainder = self.socket.read_int()?;

        if chunk_count < 0 {
            return Err(RsyncError::Unsupported(
                "Negative chunk count not supported",
            ));
        }

        let mut sums = Sums::new(chunk_count as usize, block_length, remainder, sum2_length);
        for _ in 0..chunk_count {
            let sum1 = self.socket.read_int()?;
            let sum2_data = [0u8; 4];
        }

        Ok(sums)
    }
}

#[derive(Debug, Clone)]
pub struct SumBuf {
    offset: usize,
    length: i32,
    checksum: u32,
    chain: i32,
    flags: u16,
    sum2: [u8; 16],
}

#[derive(Debug, Clone)]
pub struct Sums {
    total_file_length: usize,
    sum_chunks: Vec<SumBuf>,
    block_length: i32,
    remainder: i32,
    sum2_length: i32,
}

impl Sums {
    fn new(capacity: usize, block_length: i32, remainder: i32, sum2_length: i32) -> Self {
        Self {
            total_file_length: 0,
            sum_chunks: Vec::with_capacity(capacity),
            block_length: 0,
            remainder: 0,
            sum2_length: 0,
        }
    }
}

fn main() -> Result<(), RsyncError> {
    let mode = file_mode::Mode::new(0x1ED, 0xFFFFFFFF);
    let mut mode_dir = mode.clone();
    mode_dir.set_file_type(FileType::Directory);

    let file_list = vec![
        File {
            dirname: PathBuf::from("dot"),
            basename: PathBuf::from("."),
            modtime: 5,
            filelen: 420,
            mode: mode_dir,
            flags: FileFlags::empty(),
            name_type: NameType::DotDir,
        },
        File {
            dirname: PathBuf::from("dot"),
            basename: PathBuf::from("base_name"),
            modtime: 5,
            filelen: 0xDEADBEEF,
            mode,
            flags: FileFlags::empty(),
            name_type: NameType::Normal,
        },
    ];

    let socket = TcpListener::bind("127.0.0.1:8000").unwrap();

    loop {
        let incoming = socket.incoming().next().unwrap().unwrap();
        let socket = RsyncSocket::new(&incoming, &incoming);
        let mut rsync = Rsync {
            socket,
            checksum_seed: 1,
        };
        println!("Read init: {:?}", rsync.do_init()?);
        println!("Read query: {:?}", rsync.read_query());
        println!("Send OK: {:?}", rsync.server_send_ok());
        println!("Read args: {:?}", rsync.server_read_args());
        println!("Send compat flags: {:?}", rsync.send_compat_flags());
        println!("Send checksum seed: {:?}", rsync.send_checksum_seed());

        rsync.start_multiplexing();
        let message = rsync.read_message()?;
        println!("Receive msg: {:?}", message);
        println!("Send file list: {:?}", rsync.send_file_list(&file_list));
        println!("Receive msg: {:?}", rsync.read_message());

        // println!("Send files: {:?}", rsyncd.send_files()?);
        // println!("Send exit: {:?}", rsyncd.send_exit()?);
    }
}
