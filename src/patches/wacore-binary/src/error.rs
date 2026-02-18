use std::fmt;

use crate::jid::JidError;

#[derive(Debug)]
pub enum BinaryError {
    Io(std::io::Error),
    InvalidToken(u8),
    InvalidNode,
    NonStringKey,
    AttrParse(String),
    InvalidUtf8(std::str::Utf8Error),
    Zlib(String),
    Jid(JidError),
    UnexpectedEof,
    EmptyData,
    LeftoverData(usize),
    AttrList(Vec<BinaryError>),
}

impl fmt::Display for BinaryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BinaryError::Io(e) => write!(f, "I/O error: {e}"),
            BinaryError::InvalidToken(t) => write!(f, "Invalid token read from stream: {t}"),
            BinaryError::InvalidNode => write!(f, "Invalid node format"),
            BinaryError::NonStringKey => write!(f, "Attribute key was not a string"),
            BinaryError::AttrParse(s) => write!(f, "Attribute parsing failed: {s}"),
            BinaryError::InvalidUtf8(e) => write!(f, "Data is not valid UTF-8: {e}"),
            BinaryError::Zlib(s) => write!(f, "Zlib decompression error: {s}"),
            BinaryError::Jid(e) => write!(f, "JID parsing error: {e}"),
            BinaryError::UnexpectedEof => write!(f, "Unexpected end of binary data"),
            BinaryError::EmptyData => write!(f, "Received empty data where payload was expected"),
            BinaryError::LeftoverData(n) => write!(f, "Leftover data after decoding: {n} bytes"),
            BinaryError::AttrList(list) => write!(f, "Multiple attribute parsing errors: {list:?}"),
        }
    }
}

impl std::error::Error for BinaryError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            BinaryError::Io(e) => Some(e),
            BinaryError::InvalidUtf8(e) => Some(e),
            BinaryError::Jid(e) => Some(e),
            _ => None,
        }
    }
}

impl From<std::io::Error> for BinaryError {
    fn from(err: std::io::Error) -> Self {
        BinaryError::Io(err)
    }
}
impl From<std::str::Utf8Error> for BinaryError {
    fn from(err: std::str::Utf8Error) -> Self {
        BinaryError::InvalidUtf8(err)
    }
}
impl From<JidError> for BinaryError {
    fn from(err: JidError) -> Self {
        BinaryError::Jid(err)
    }
}
impl Clone for BinaryError {
    fn clone(&self) -> Self {
        match self {
            BinaryError::Io(e) => BinaryError::Io(std::io::Error::new(e.kind(), e.to_string())),
            BinaryError::InvalidToken(u) => BinaryError::InvalidToken(*u),
            BinaryError::InvalidNode => BinaryError::InvalidNode,
            BinaryError::NonStringKey => BinaryError::NonStringKey,
            BinaryError::AttrParse(s) => BinaryError::AttrParse(s.clone()),
            BinaryError::InvalidUtf8(e) => BinaryError::InvalidUtf8(*e),
            BinaryError::Zlib(s) => BinaryError::Zlib(s.clone()),
            BinaryError::Jid(e) => BinaryError::Jid(JidError::InvalidFormat(e.to_string())),
            BinaryError::UnexpectedEof => BinaryError::UnexpectedEof,
            BinaryError::EmptyData => BinaryError::EmptyData,
            BinaryError::LeftoverData(n) => BinaryError::LeftoverData(*n),
            BinaryError::AttrList(list) => BinaryError::AttrList(list.clone()),
        }
    }
}

pub type Result<T> = std::result::Result<T, BinaryError>;
