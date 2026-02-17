#![feature(portable_simd)]

pub mod attrs;
pub mod builder;
pub mod consts;
mod decoder;
mod encoder;
pub mod error;
pub mod jid;
pub mod marshal;
pub mod node;
pub mod token;
pub mod util;

pub use attrs::{AttrParser, AttrParserRef};
pub use error::{BinaryError, Result};
pub use marshal::{marshal, marshal_ref, marshal_ref_to, marshal_to};
pub use node::{Node, NodeRef};
