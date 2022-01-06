use std::{
    io::{BufRead, BufReader, Read, Write},
    ops::Deref,
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
}

impl From<std::io::Error> for ReceiveError {
    fn from(from: std::io::Error) -> Self {
        Self::Io(from)
    }
}

pub struct RsyncSocket<R, W>
where
    R: Read,
    W: Write,
{
    is_multiplexed: bool,
    read: BufReader<R>,
    write: W,
}

impl<R, W> RsyncSocket<R, W>
where
    R: Read,
    W: Write,
{
    pub fn new(read: R, write: W) -> Self {
        Self {
            is_multiplexed: false,
            read: BufReader::new(read),
            write,
        }
    }

    pub fn is_multiplexed(&self) -> bool {
        self.is_multiplexed
    }
    pub fn set_multiplexed(&mut self, multiplexed: bool) {
        self.is_multiplexed = multiplexed;
    }

    fn msg_to_header<T: Deref<Target = [u8]>>(message: &RsyncMessage<T>) -> [u8; 4] {
        let mut data = [0u8; 4];
        match message {
            RsyncMessage::Data { length, data: _ } => {
                let tag = 0x07;
                data[3] = tag;
                data[2] = ((length >> 16) as u8) & 0xFF;
                data[1] = ((length >> 8) as u8) & 0xFF;
                data[0] = (*length as u8) & 0xFF;
            }
            _ => {}
        }
        data
    }

    fn msg_from_header(data: &[u8]) -> (u8, u32) {
        let data_bytes: u32 = (data[2] as u32) << 16 | (data[1] as u32) << 8 | data[0] as u32;
        let tag = data[3].wrapping_sub(0x07);

        (tag, data_bytes)
    }

    pub fn send_message<T: Deref<Target = [u8]>>(
        &mut self,
        message: &RsyncMessage<T>,
    ) -> Result<(), SendError> {
        if !self.is_multiplexed {
            if let RsyncMessage::Data { length: _, data } = message {
                self.write.write(&data)?;
                Ok(())
            } else {
                Err(SendError::NonDataWhileMultiplexed)
            }
        } else {
            self.write.write(&Self::msg_to_header(message))?;
            match message {
                RsyncMessage::Data { length: _, data } => {
                    self.write.write(&data)?;
                }
                _ => {}
            }
            Ok(())
        }
    }

    pub fn read_message(&mut self) -> Result<RsyncMessage<Vec<u8>>, ReceiveError> {
        if !self.is_multiplexed {
            let mut data = Vec::new();
            self.read.read_to_end(&mut data)?;
            Ok(RsyncMessage::Data {
                length: data.len(),
                data,
            })
        } else {
            let mut data = [0u8; 4];
            self.read.read_exact(&mut data)?;
            let (tag, data) = Self::msg_from_header(&data);

            match tag {
                0x00 => {
                    let mut data_bytes = vec![0; data as usize];
                    self.read.read_exact(&mut data_bytes)?;
                    Ok(RsyncMessage::Data {
                        length: data as usize,
                        data: data_bytes,
                    })
                }
                _ => Err(ReceiveError::UnsupportedMessageType),
            }
        }
    }

    pub fn read_raw_until(&mut self, delimiter: u8) -> Result<Vec<u8>, ReceiveError> {
        if !self.is_multiplexed {
            let mut data = Vec::with_capacity(256);
            self.read.read_until(delimiter, &mut data)?;
            Ok(data)
        } else {
            Err(ReceiveError::RawReadWhileMultiplexed)
        }
    }

    pub fn send_data<T: Deref<Target = [u8]>>(&mut self, data: T) -> Result<(), SendError> {
        self.send_message(&RsyncMessage::Data {
            length: data.len(),
            data,
        })
    }

    pub fn read_int(&mut self) -> Result<i32, ReceiveError> {
        let mut data = [0u8; 4];
        self.read.read_exact(&mut data)?;
        Ok(i32::from_le_bytes(data))
    }
}

#[derive(Debug, Clone)]
pub enum RsyncMessage<T>
where
    T: Deref<Target = [u8]>,
{
    Data { length: usize, data: T },
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
