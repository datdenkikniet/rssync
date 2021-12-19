use std::{
    fmt::Write as FmtWrite,
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    thread::sleep,
    time::Duration,
};

const VER_MAJOR: u32 = 31;
const VER_MINOR: u32 = 0;
const INTERNAL_BUFFER_SIZE: usize = 2048;
const MAX_LINE_LENGTH: usize = 64;
const VERSION: Version = Version {
    major: VER_MAJOR,
    minor: VER_MINOR,
};

pub struct Rsyncd {
    buffer: [u8; INTERNAL_BUFFER_SIZE],
    tcp_stream: TcpStream,
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

    pub fn serialize(&self, string: &mut String) {
        write!(string, "{}.{}", self.major, self.minor).ok();
    }
}

#[derive(Debug, Clone)]
pub enum Query {
    ModuleList,
}

#[derive(Debug, Clone)]
pub enum QueryParseError {
    InvalidQuery,
}

impl Query {
    pub fn parse(data: &str) -> Result<Self, QueryParseError> {
        if data == "" {
            Ok(Self::ModuleList)
        } else {
            Err(QueryParseError::InvalidQuery)
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
}

#[derive(Debug)]
pub enum RsyncdError {
    Io(std::io::Error),
    ClientError(ClientError),
    LineTooLarge,
}

impl From<std::io::Error> for RsyncdError {
    fn from(io: std::io::Error) -> Self {
        Self::Io(io)
    }
}

impl Rsyncd {
    /// Read a line
    ///
    /// Doesn't strip newline character
    fn read_line(&mut self) -> Result<String, RsyncdError> {
        let mut total_data = 0;
        while total_data < MAX_LINE_LENGTH && total_data < INTERNAL_BUFFER_SIZE {
            let writable_slice = &mut self.buffer[total_data..];

            let read_data = self.tcp_stream.read(writable_slice)?;

            total_data += read_data;
            if writable_slice[..read_data].contains(&0x0A) {
                break;
            }
        }

        if total_data > INTERNAL_BUFFER_SIZE {
            Err(RsyncdError::LineTooLarge)
        } else {
            Ok(String::from_utf8_lossy(&self.buffer[..total_data]).to_string())
        }
    }

    fn read_init(&mut self) -> Result<Version, RsyncdError> {
        let result = self.read_line()?;

        // Strip newline
        let result = result.trim();

        if result.starts_with("@RSYNCD: ") {
            let (_, version) = result.split_at(9);
            Ok(Version::parse(version)
                .map_err(|err| RsyncdError::ClientError(ClientError::VersionParseError(err)))?)
        } else {
            Err(RsyncdError::ClientError(ClientError::InvalidHeader))
        }
    }

    fn send_init(&mut self) -> Result<(), RsyncdError> {
        let mut value = String::with_capacity(64);

        value.push_str("@RSYNCD: ");
        VERSION.serialize(&mut value);
        value.push_str("\n");
        self.tcp_stream.write(value.as_bytes())?;

        Ok(())
    }

    fn read_query(&mut self) -> Result<Query, RsyncdError> {
        let result = self.read_line()?;
        // Strip newline
        let query = result.trim();
        Query::parse(query).map_err(|err| RsyncdError::ClientError(ClientError::InvalidQuery(err)))
    }

    fn send_module_list(&mut self) -> Result<(), RsyncdError> {
        let mut value = String::with_capacity(128);
        value.push_str("module_one\nmodule_two\nmodule_three\n");
        self.tcp_stream.write(value.as_bytes())?;
        Ok(())
    }

    fn send_exit(&mut self) -> Result<(), RsyncdError> {
        let mut value = String::with_capacity(128);
        value.push_str("@RSYNCD: EXIT\n");
        self.tcp_stream.write(value.as_bytes())?;
        Ok(())
    }
}

fn main() -> Result<(), RsyncdError> {
    let socket = TcpListener::bind("127.0.0.1:8000").unwrap();

    let incoming = socket.incoming().next().unwrap().unwrap();

    let mut rsyncd = Rsyncd {
        buffer: [0u8; 2048],
        tcp_stream: incoming,
    };
    rsyncd.send_init().ok();
    println!("{:?}", rsyncd.read_init()?);
    println!("{:?}", rsyncd.read_query()?);
    println!("{:?}", rsyncd.send_module_list()?);
    println!("{:?}", rsyncd.send_exit()?);

    Ok(())
}
