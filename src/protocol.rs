use std::{
    io::{BufRead, BufReader, Read, Write},
    net::TcpStream,
};

#[derive(Debug)]
pub enum SendError {
    Io(std::io::Error),
    NonDataWhileMultiplexed,
}

impl From<std::io::Error> for SendError {
    fn from(from: std::io::Error) -> Self {
        Self::Io(from)
    }
}

#[derive(Debug)]
pub enum ReceiveError {
    Io(std::io::Error),
    ReadWhileMultiplexed,
    UnsupportedMessageType,
    RawReadWhileMultiplexed,
    ReadMessageWhileNotMultiplexed,
    Invalid,
}

impl From<std::io::Error> for ReceiveError {
    fn from(from: std::io::Error) -> Self {
        Self::Io(from)
    }
}

pub struct RsyncSocket<'a> {
    multiplex_in: bool,
    multiplex_out: bool,
    read: BufReader<&'a TcpStream>,
    write: &'a TcpStream,
}

impl<'a> RsyncSocket<'a> {
    pub fn new(stream: &'a TcpStream) -> Self {
        Self {
            multiplex_in: false,
            multiplex_out: false,
            read: BufReader::new(stream),
            write: stream,
        }
    }

    pub fn multiplex_out(&self) -> bool {
        self.multiplex_out
    }
    pub fn set_multiplex_out(&mut self, mp: bool) {
        self.multiplex_out = mp
    }

    pub fn multiplex_in(&self) -> bool {
        self.multiplex_out
    }
    pub fn set_multiplex_in(&mut self, mp: bool) {
        self.multiplex_in = mp
    }

    fn msg_to_header(message: &RsyncMessage) -> [u8; 4] {
        let mut header_data = [0u8; 4];
        match message {
            RsyncMessage::Data { data } => {
                let length = data.len();
                let tag = 0x07;
                header_data[3] = tag;
                header_data[2] = ((length >> 16) as u8) & 0xFF;
                header_data[1] = ((length >> 8) as u8) & 0xFF;
                header_data[0] = (length as u8) & 0xFF;
            }
            _ => {}
        }
        header_data
    }

    fn msg_from_header(data: &[u8]) -> (u8, u32) {
        let data_bytes: u32 = (data[2] as u32) << 16 | (data[1] as u32) << 8 | data[0] as u32;
        let tag = data[3].wrapping_sub(0x07);

        (tag, data_bytes)
    }

    pub fn send_message(&mut self, message: &RsyncMessage) -> Result<(), SendError> {
        if !self.multiplex_out {
            if let RsyncMessage::Data { data } = message {
                self.write.write(&data)?;
                Ok(())
            } else {
                Err(SendError::NonDataWhileMultiplexed)
            }
        } else {
            self.write.write(&Self::msg_to_header(message))?;
            match message {
                RsyncMessage::Data { data } => {
                    self.write.write(&data)?;
                }
                _ => {
                    todo!();
                }
            }
            Ok(())
        }
    }

    pub fn read_message(&mut self) -> Result<RsyncMessage, ReceiveError> {
        if !self.multiplex_in {
            return Err(ReceiveError::ReadMessageWhileNotMultiplexed);
        } else {
            let mut data = [0u8; 4];
            self.read.read_exact(&mut data)?;
            let (tag, data) = Self::msg_from_header(&data);

            match tag {
                0x00 => {
                    let mut data_bytes = vec![0; data as usize];
                    self.read.read_exact(&mut data_bytes)?;
                    Ok(RsyncMessage::Data { data: data_bytes })
                }
                _ => Err(ReceiveError::UnsupportedMessageType),
            }
        }
    }

    pub fn read_raw_until(&mut self, delimiter: u8) -> Result<Vec<u8>, ReceiveError> {
        if !self.multiplex_in {
            let mut data = Vec::with_capacity(256);
            self.read.read_until(delimiter, &mut data)?;
            Ok(data)
        } else {
            Err(ReceiveError::RawReadWhileMultiplexed)
        }
    }

    pub fn send_data(&mut self, data: &[u8]) -> Result<(), SendError> {
        self.send_message(&RsyncMessage::Data {
            data: data.iter().map(|val| *val).collect(),
        })
    }

    pub fn read_int(&mut self) -> Result<i32, ReceiveError> {
        if let RsyncMessage::Data { data } = self.read_message()? {
            let mut data_bytes = [0u8; 4];
            println!("Data: {:X?}", data);
            for i in 0..4 {
                data_bytes[i] = data[i];
            }
            Ok(i32::from_be_bytes(data_bytes))
        } else {
            Err(ReceiveError::Invalid)
        }
    }
}

#[derive(Debug, Clone)]
pub enum RsyncMessage {
    Data { data: Vec<u8> },
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
