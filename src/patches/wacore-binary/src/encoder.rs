use std::io::Write;

use core::simd::prelude::*;
use core::simd::{Simd, u8x16};
use std::simd::Select;

use crate::error::Result;
use crate::jid;
use crate::node::{Attrs, AttrsRef, Node, NodeContent, NodeContentRef, NodeRef};
use crate::token;

struct ParsedJid<'a> {
    user: &'a str,
    server: &'a str,
    domain_type: u8,
    device: Option<u16>,
}

fn parse_jid(input: &str) -> Option<ParsedJid<'_>> {
    let sep_idx = input.find('@')?;
    let server = &input[sep_idx + 1..];
    let user_combined = &input[..sep_idx];

    let (user_agent, device) = match user_combined.split_once(':') {
        Some((ua, device_part)) => {
            let parsed_device = if device_part.is_empty() {
                None
            } else {
                device_part.parse::<u16>().ok()
            };
            (ua, parsed_device)
        }
        None => (user_combined, None),
    };

    let (user, agent_override) = match user_agent.split_once('_') {
        Some((u, agent_part)) => (u, agent_part.parse::<u16>().ok()),
        None => (user_agent, None),
    };

    let agent_byte = agent_override.unwrap_or(0) as u8;
    let domain_type = if server == jid::HIDDEN_USER_SERVER {
        1
    } else if server == jid::HOSTED_SERVER {
        128
    } else if server == "hosted.lid" {
        129
    } else {
        agent_byte
    };

    Some(ParsedJid {
        user,
        server,
        domain_type,
        device,
    })
}

pub(crate) struct Encoder<W: Write> {
    writer: W,
}

impl<W: Write> Encoder<W> {
    pub(crate) fn new(writer: W) -> Result<Self> {
        let mut enc = Self { writer };
        enc.write_u8(0)?;
        Ok(enc)
    }

    fn write_u8(&mut self, val: u8) -> Result<()> {
        self.writer.write_all(&[val])?;
        Ok(())
    }

    fn write_u16_be(&mut self, val: u16) -> Result<()> {
        self.writer.write_all(&val.to_be_bytes())?;
        Ok(())
    }

    fn write_u32_be(&mut self, val: u32) -> Result<()> {
        self.writer.write_all(&val.to_be_bytes())?;
        Ok(())
    }

    fn write_u20_be(&mut self, value: u32) -> Result<()> {
        self.writer.write_all(&[((value >> 16) & 0x0F) as u8])?;
        self.writer.write_all(&[((value >> 8) & 0xFF) as u8])?;
        self.writer.write_all(&[(value & 0xFF) as u8])?;
        Ok(())
    }

    fn write_raw_bytes(&mut self, bytes: &[u8]) -> Result<()> {
        self.writer.write_all(bytes)?;
        Ok(())
    }

    fn write_bytes_with_len(&mut self, bytes: &[u8]) -> Result<()> {
        let len = bytes.len();
        if len < 256 {
            self.write_u8(token::BINARY_8)?;
            self.write_u8(len as u8)?;
        } else if len < (1 << 20) {
            self.write_u8(token::BINARY_20)?;
            self.write_u20_be(len as u32)?;
        } else {
            self.write_u8(token::BINARY_32)?;
            self.write_u32_be(len as u32)?;
        }
        self.write_raw_bytes(bytes)
    }

    fn write_string(&mut self, s: &str) -> Result<()> {
        // Empty strings must be encoded as BINARY_8 + 0
        if s.is_empty() {
            self.write_u8(token::BINARY_8)?;
            self.write_u8(0)?;
            return Ok(());
        }

        if let Some(token) = token::index_of_single_token(s) {
            self.write_u8(token)?;
        } else if let Some((dict, token)) = token::index_of_double_byte_token(s) {
            self.write_u8(token::DICTIONARY_0 + dict)?;
            self.write_u8(token)?;
        } else if Self::validate_nibble(s) {
            self.write_packed_bytes(s, token::NIBBLE_8)?;
        } else if Self::validate_hex(s) {
            self.write_packed_bytes(s, token::HEX_8)?;
        } else if let Some(jid) = parse_jid(s) {
            self.write_jid(&jid)?;
        } else {
            self.write_bytes_with_len(s.as_bytes())?;
        }
        Ok(())
    }

    fn write_jid(&mut self, jid: &ParsedJid<'_>) -> Result<()> {
        if let Some(device) = jid.device {
            self.write_u8(token::AD_JID)?;
            self.write_u8(jid.domain_type)?;
            self.write_u8(device as u8)?;
            self.write_string(jid.user)?;
        } else {
            self.write_u8(token::JID_PAIR)?;
            if jid.user.is_empty() {
                self.write_u8(token::LIST_EMPTY)?;
            } else {
                self.write_string(jid.user)?;
            }
            self.write_string(jid.server)?;
        }
        Ok(())
    }

    fn validate_nibble(value: &str) -> bool {
        if value.len() > token::PACKED_MAX as usize {
            return false;
        }
        value
            .chars()
            .all(|c| c.is_ascii_digit() || c == '-' || c == '.')
    }

    fn pack_nibble(value: char) -> u8 {
        match value {
            '-' => 10,
            '.' => 11,
            '\x00' => 15,
            c if c.is_ascii_digit() => c as u8 - b'0',
            _ => panic!("Invalid char for nibble packing: {value}"),
        }
    }

    fn validate_hex(value: &str) -> bool {
        if value.len() > token::PACKED_MAX as usize {
            return false;
        }
        value
            .chars()
            .all(|c| c.is_ascii_hexdigit() && (c.is_ascii_uppercase() || c.is_ascii_digit()))
    }

    fn pack_hex(value: char) -> u8 {
        match value {
            c if c.is_ascii_digit() => c as u8 - b'0',
            c if ('A'..='F').contains(&c) => 10 + (c as u8 - b'A'),
            '\x00' => 15,
            _ => panic!("Invalid char for hex packing: {value}"),
        }
    }

    fn pack_byte_pair(&self, packer: fn(char) -> u8, part1: char, part2: char) -> u8 {
        (packer(part1) << 4) | packer(part2)
    }

    fn write_packed_bytes(&mut self, value: &str, data_type: u8) -> Result<()> {
        if value.len() > token::PACKED_MAX as usize {
            panic!("String too long to be packed: {}", value.len());
        }

        self.write_u8(data_type)?;

        let mut rounded_len = value.len().div_ceil(2) as u8;
        if !value.len().is_multiple_of(2) {
            rounded_len |= 0x80;
        }
        self.write_u8(rounded_len)?;

        let mut input_bytes = value.as_bytes();

        while input_bytes.len() >= 16 {
            let (chunk, rest) = input_bytes.split_at(16);
            let input = u8x16::from_slice(chunk);

            let mut nibbles = if data_type == token::NIBBLE_8 {
                let indices = input.saturating_sub(Simd::splat(b'-'));
                const LOOKUP: [u8; 16] = [10, 11, 255, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 255, 255, 255];
                Simd::from_array(LOOKUP).swizzle_dyn(indices)
            } else {
                let ascii_0 = Simd::splat(b'0');
                let ascii_a = Simd::splat(b'A');
                let ten = Simd::splat(10);

                let digit_vals = input - ascii_0;
                let letter_vals = input - ascii_a + ten;
                let is_letter = input.simd_ge(ascii_a);
                is_letter.select(letter_vals, digit_vals)
            };

            if data_type == token::NIBBLE_8 {
                let pad_mask = input.simd_eq(Simd::splat(b'\x00'));
                nibbles = pad_mask.select(Simd::splat(15), nibbles);
            }

            let (evens, odds) = nibbles.deinterleave(nibbles.rotate_elements_left::<1>());
            let packed: Simd<u8, 16> = (evens << Simd::splat(4)) | odds;
            let packed_bytes = packed.to_array();
            self.write_raw_bytes(&packed_bytes[..8])?;

            input_bytes = rest;
        }

        let packer: fn(char) -> u8 = if data_type == token::NIBBLE_8 {
            Self::pack_nibble
        } else {
            Self::pack_hex
        };

        let mut chars = core::str::from_utf8(input_bytes)?.chars();
        while let Some(part1) = chars.next() {
            let part2 = chars.next().unwrap_or('\x00');
            self.write_u8(self.pack_byte_pair(packer, part1, part2))?;
        }
        Ok(())
    }

    fn write_list_start(&mut self, len: usize) -> Result<()> {
        if len == 0 {
            self.write_u8(token::LIST_EMPTY)?;
        } else if len < 256 {
            self.write_u8(248)?;
            self.write_u8(len as u8)?;
        } else {
            self.write_u8(249)?;
            self.write_u16_be(len as u16)?;
        }
        Ok(())
    }

    fn write_attributes(&mut self, attrs: &Attrs) -> Result<()> {
        for (key, value) in attrs {
            self.write_string(key)?;
            self.write_string(value)?;
        }
        Ok(())
    }

    fn write_content(&mut self, content: &NodeContent) -> Result<()> {
        match content {
            NodeContent::String(s) => self.write_string(s)?,
            NodeContent::Bytes(bytes) => self.write_bytes_with_len(bytes)?,
            NodeContent::Nodes(nodes) => {
                self.write_list_start(nodes.len())?;
                for node in nodes {
                    self.write_node(node)?;
                }
            }
        }
        Ok(())
    }

    pub(crate) fn write_node(&mut self, node: &Node) -> Result<()> {
        let content_len = if node.content.is_some() { 1 } else { 0 };
        let list_len = 1 + (node.attrs.len() * 2) + content_len;

        self.write_list_start(list_len)?;
        self.write_string(&node.tag)?;
        self.write_attributes(&node.attrs)?;

        if let Some(content) = &node.content {
            self.write_content(content)?;
        }
        Ok(())
    }

    /// Zero-copy serialization of a borrowed `NodeRef`.
    /// This avoids the need to call `.to_owned()` when encoding nodes that were just parsed.
    pub(crate) fn write_node_ref(&mut self, node: &NodeRef<'_>) -> Result<()> {
        let content_len = if node.content.is_some() { 1 } else { 0 };
        let list_len = 1 + (node.attrs.len() * 2) + content_len;

        self.write_list_start(list_len)?;
        self.write_string(&node.tag)?;
        self.write_attributes_ref(&node.attrs)?;

        if let Some(content) = node.content.as_deref() {
            self.write_content_ref(content)?;
        }
        Ok(())
    }

    /// Write attributes from a borrowed AttrsRef (Vec of key-value pairs).
    fn write_attributes_ref(&mut self, attrs: &AttrsRef<'_>) -> Result<()> {
        for (key, value) in attrs {
            self.write_string(key)?;
            // ValueRef can be either a string or a JID - convert to string representation
            let value_str = value.to_string_cow();
            self.write_string(&value_str)?;
        }
        Ok(())
    }

    /// Write content from a borrowed NodeContentRef.
    fn write_content_ref(&mut self, content: &NodeContentRef<'_>) -> Result<()> {
        match content {
            NodeContentRef::String(s) => self.write_string(s)?,
            NodeContentRef::Bytes(bytes) => self.write_bytes_with_len(bytes)?,
            NodeContentRef::Nodes(nodes) => {
                self.write_list_start(nodes.len())?;
                for node in nodes.iter() {
                    self.write_node_ref(node)?;
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    type TestResult = crate::error::Result<()>;

    #[test]
    fn test_encode_node() -> TestResult {
        let node = Node::new(
            "message",
            indexmap::IndexMap::new(),
            Some(NodeContent::String("receipt".to_string())),
        );

        let mut buffer = Vec::new();
        let mut encoder = Encoder::new(Cursor::new(&mut buffer))?;
        encoder.write_node(&node)?;

        let expected = vec![0, 248, 2, 19, 7];
        assert_eq!(buffer, expected);
        assert_eq!(buffer.len(), 5);
        Ok(())
    }

    #[test]
    fn test_nibble_packing() -> TestResult {
        // Test string with nibble characters: '-', '.', '0'-'9'
        let test_str = "-.0123456789";
        let node = Node::new(
            "test",
            indexmap::IndexMap::new(),
            Some(NodeContent::String(test_str.to_string())),
        );

        let mut buffer = Vec::new();
        let mut encoder = Encoder::new(Cursor::new(&mut buffer))?;
        encoder.write_node(&node)?;

        let expected = vec![
            0, 248, 2, 252, 4, 116, 101, 115, 116, 255, 6, 171, 1, 35, 69, 103, 137,
        ];
        assert_eq!(buffer, expected);
        assert_eq!(buffer.len(), 17);
        Ok(())
    }

    /// Test LIST_8 boundary (length 255)
    #[test]
    fn test_list_size_list8_boundary() -> TestResult {
        let mut buffer = Vec::new();
        let mut encoder = Encoder::new(Cursor::new(&mut buffer))?;

        // LIST_8 should be used for lengths 1-255
        encoder.write_list_start(255)?;

        // Expected: LIST_8 (248), then length 255
        assert_eq!(buffer[1], token::LIST_8);
        assert_eq!(buffer[2], 255);
        Ok(())
    }

    /// Test LIST_16 boundary (length 256)
    #[test]
    fn test_list_size_list16_boundary() -> TestResult {
        let mut buffer = Vec::new();
        let mut encoder = Encoder::new(Cursor::new(&mut buffer))?;

        // LIST_16 should be used for lengths 256+
        encoder.write_list_start(256)?;

        // Expected: LIST_16 (249), then length as u16 big-endian
        assert_eq!(buffer[1], token::LIST_16);
        assert_eq!(buffer[2], 0x01); // 256 >> 8
        assert_eq!(buffer[3], 0x00); // 256 & 0xFF
        Ok(())
    }

    /// Test empty list encoding
    #[test]
    fn test_list_size_empty() -> TestResult {
        let mut buffer = Vec::new();
        let mut encoder = Encoder::new(Cursor::new(&mut buffer))?;

        encoder.write_list_start(0)?;

        // Empty list uses LIST_EMPTY token
        assert_eq!(buffer[1], token::LIST_EMPTY);
        Ok(())
    }

    /// Test hex packing validation
    #[test]
    fn test_hex_validation() {
        // Valid hex strings (uppercase A-F, digits 0-9)
        assert!(Encoder::<Vec<u8>>::validate_hex("0123456789ABCDEF"));
        assert!(Encoder::<Vec<u8>>::validate_hex("DEADBEEF"));
        assert!(Encoder::<Vec<u8>>::validate_hex("1234"));

        // Invalid: lowercase letters
        assert!(!Encoder::<Vec<u8>>::validate_hex("abcdef"));
        assert!(!Encoder::<Vec<u8>>::validate_hex("DeadBeef"));

        // Invalid: special characters
        assert!(!Encoder::<Vec<u8>>::validate_hex("-"));
        assert!(!Encoder::<Vec<u8>>::validate_hex("."));
        assert!(!Encoder::<Vec<u8>>::validate_hex(" "));

        // Empty string is valid (but will be encoded as regular string)
        assert!(Encoder::<Vec<u8>>::validate_hex(""));
    }

    /// Test nibble packing validation
    #[test]
    fn test_nibble_validation() {
        // Valid nibble strings: digits, dash, dot
        assert!(Encoder::<Vec<u8>>::validate_nibble("0123456789"));
        assert!(Encoder::<Vec<u8>>::validate_nibble("-"));
        assert!(Encoder::<Vec<u8>>::validate_nibble("."));
        assert!(Encoder::<Vec<u8>>::validate_nibble("123-456.789"));

        // Invalid: letters
        assert!(!Encoder::<Vec<u8>>::validate_nibble("abc"));
        assert!(!Encoder::<Vec<u8>>::validate_nibble("123abc"));

        // Invalid: uppercase letters
        assert!(!Encoder::<Vec<u8>>::validate_nibble("ABC"));

        // Invalid: special characters other than - and .
        assert!(!Encoder::<Vec<u8>>::validate_nibble("123!456"));
        assert!(!Encoder::<Vec<u8>>::validate_nibble("@"));
    }

    /// Test BINARY_8, BINARY_20, BINARY_32 boundary transitions
    #[test]
    fn test_binary_length_boundaries() -> TestResult {
        // BINARY_8: length < 256
        let short_data = vec![0x42; 255];
        let mut buffer = Vec::new();
        let mut encoder = Encoder::new(Cursor::new(&mut buffer))?;
        encoder.write_bytes_with_len(&short_data)?;
        assert_eq!(buffer[1], token::BINARY_8);
        assert_eq!(buffer[2], 255);

        // BINARY_20: 256 <= length < 2^20
        let medium_data = vec![0x42; 256];
        let mut buffer = Vec::new();
        let mut encoder = Encoder::new(Cursor::new(&mut buffer))?;
        encoder.write_bytes_with_len(&medium_data)?;
        assert_eq!(buffer[1], token::BINARY_20);
        // 256 in u20 big-endian: 0x00, 0x01, 0x00
        assert_eq!(buffer[2], 0x00);
        assert_eq!(buffer[3], 0x01);
        assert_eq!(buffer[4], 0x00);

        Ok(())
    }

    /// Test node with many children uses correct list encoding
    #[test]
    fn test_node_with_255_children() -> TestResult {
        let children: Vec<Node> = (0..255)
            .map(|_| Node::new("child", indexmap::IndexMap::new(), None))
            .collect();

        let parent = Node::new(
            "parent",
            indexmap::IndexMap::new(),
            Some(NodeContent::Nodes(children)),
        );

        let mut buffer = Vec::new();
        let mut encoder = Encoder::new(Cursor::new(&mut buffer))?;
        encoder.write_node(&parent)?;

        // Should encode successfully with LIST_8 for children
        assert!(!buffer.is_empty());
        Ok(())
    }

    /// Test node with 256 children uses LIST_16
    #[test]
    fn test_node_with_256_children() -> TestResult {
        let children: Vec<Node> = (0..256)
            .map(|_| Node::new("x", indexmap::IndexMap::new(), None))
            .collect();

        let parent = Node::new(
            "parent",
            indexmap::IndexMap::new(),
            Some(NodeContent::Nodes(children)),
        );

        let mut buffer = Vec::new();
        let mut encoder = Encoder::new(Cursor::new(&mut buffer))?;
        encoder.write_node(&parent)?;

        // Should encode successfully with LIST_16 for children
        assert!(!buffer.is_empty());
        Ok(())
    }

    /// Test string at PACKED_MAX boundary (127 chars)
    #[test]
    fn test_packed_max_boundary() {
        // Exactly PACKED_MAX characters should be valid for packing
        let max_nibble = "0".repeat(token::PACKED_MAX as usize);
        assert!(Encoder::<Vec<u8>>::validate_nibble(&max_nibble));

        // One more than PACKED_MAX should NOT be packed
        let over_max = "0".repeat(token::PACKED_MAX as usize + 1);
        assert!(!Encoder::<Vec<u8>>::validate_nibble(&over_max));
    }

    /// Test empty string encoding - should be BINARY_8 + 0, not just 0
    #[test]
    fn test_empty_string_encoding() -> TestResult {
        let mut buffer = Vec::new();
        let mut encoder = Encoder::new(Cursor::new(&mut buffer))?;
        encoder.write_string("")?;

        // According to WhatsApp web protocol:
        // Empty string should be encoded as BINARY_8 (252) + 0
        // NOT as token 0 (LIST_EMPTY)
        println!("Empty string encoding: {:?}", &buffer[1..]);
        assert_eq!(
            buffer.len(),
            3,
            "Empty string should encode to 2 bytes (plus leading 0)"
        );
        assert_eq!(
            buffer[1],
            token::BINARY_8,
            "First byte should be BINARY_8 (252)"
        );
        assert_eq!(buffer[2], 0, "Second byte should be 0 (length)");
        Ok(())
    }

    /// Test encode/decode round-trip for empty string in node attributes
    #[test]
    fn test_empty_string_roundtrip() -> TestResult {
        use crate::decoder::Decoder;

        let mut attrs = indexmap::IndexMap::new();
        attrs.insert("key".to_string(), "".to_string()); // Empty value
        attrs.insert("".to_string(), "value".to_string()); // Empty key

        let node = Node::new("test", attrs, Some(NodeContent::String("".to_string())));

        let mut buffer = Vec::new();
        let mut encoder = Encoder::new(Cursor::new(&mut buffer))?;
        encoder.write_node(&node)?;

        let mut decoder = Decoder::new(&buffer[1..]);
        let decoded = decoder.read_node_ref()?.to_owned();

        assert_eq!(decoded.tag, "test");
        assert_eq!(decoded.attrs.get("key"), Some(&"".to_string()));
        assert_eq!(decoded.attrs.get(""), Some(&"value".to_string()));

        // Empty strings are encoded as BINARY_8 + 0, which decodes as empty bytes
        match &decoded.content {
            Some(NodeContent::Bytes(b)) => assert!(b.is_empty(), "Content should be empty bytes"),
            other => panic!("Expected empty bytes, got {:?}", other),
        }
        Ok(())
    }
}
