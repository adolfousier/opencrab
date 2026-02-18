use crate::error::{BinaryError, Result};
use crate::jid::JidRef;
use crate::node::{AttrsRef, NodeContentRef, NodeRef, NodeVec, ValueRef};
use crate::token;
use std::borrow::Cow;
use std::simd::{Simd, prelude::*, u8x16};

pub(crate) struct Decoder<'a> {
    data: &'a [u8],
    position: usize,
}

impl<'a> Decoder<'a> {
    pub(crate) fn new(data: &'a [u8]) -> Self {
        Self { data, position: 0 }
    }

    pub(crate) fn is_finished(&self) -> bool {
        self.position >= self.data.len()
    }

    pub(crate) fn bytes_left(&self) -> usize {
        self.data.len() - self.position
    }

    fn check_eos(&self, len: usize) -> Result<()> {
        if self.bytes_left() >= len {
            Ok(())
        } else {
            Err(BinaryError::UnexpectedEof)
        }
    }

    fn read_u8(&mut self) -> Result<u8> {
        self.check_eos(1)?;
        let value = self.data[self.position];
        self.position += 1;
        Ok(value)
    }

    fn read_u16_be(&mut self) -> Result<u16> {
        self.check_eos(2)?;
        let value = u16::from_be_bytes([self.data[self.position], self.data[self.position + 1]]);
        self.position += 2;
        Ok(value)
    }

    fn read_u20_be(&mut self) -> Result<u32> {
        self.check_eos(3)?;
        let bytes = [
            self.data[self.position],
            self.data[self.position + 1],
            self.data[self.position + 2],
        ];
        self.position += 3;
        Ok(((bytes[0] as u32 & 0x0F) << 16) | ((bytes[1] as u32) << 8) | (bytes[2] as u32))
    }

    fn read_u32_be(&mut self) -> Result<u32> {
        self.check_eos(4)?;
        let value = u32::from_be_bytes([
            self.data[self.position],
            self.data[self.position + 1],
            self.data[self.position + 2],
            self.data[self.position + 3],
        ]);
        self.position += 4;
        Ok(value)
    }

    fn read_bytes(&mut self, len: usize) -> Result<&'a [u8]> {
        self.check_eos(len)?;
        let slice = &self.data[self.position..self.position + len];
        self.position += len;
        Ok(slice)
    }

    fn read_string(&mut self, len: usize) -> Result<Cow<'a, str>> {
        let bytes = self.read_bytes(len)?;
        match std::str::from_utf8(bytes) {
            Ok(s) => Ok(Cow::Borrowed(s)),
            Err(e) => Err(BinaryError::InvalidUtf8(e)),
        }
    }

    fn read_list_size(&mut self, tag: u8) -> Result<usize> {
        match tag {
            token::LIST_EMPTY => Ok(0),
            248 => self.read_u8().map(|v| v as usize),
            249 => self.read_u16_be().map(|v| v as usize),
            _ => Err(BinaryError::InvalidToken(tag)),
        }
    }

    fn read_jid_pair(&mut self) -> Result<JidRef<'a>> {
        let user_val = self.read_value_as_string()?;
        let server = self.read_value_as_string()?.unwrap_or(Cow::Borrowed(""));
        let user = user_val.unwrap_or(Cow::Borrowed(""));
        Ok(JidRef {
            user,
            server,
            agent: 0,
            device: 0,
            integrator: 0,
        })
    }

    fn read_ad_jid(&mut self) -> Result<JidRef<'a>> {
        let agent = self.read_u8()?;
        let device = self.read_u8()? as u16;
        let user = self
            .read_value_as_string()?
            .ok_or(BinaryError::InvalidNode)?;

        let server = match agent {
            0 => Cow::Borrowed(crate::jid::DEFAULT_USER_SERVER),
            1 => Cow::Borrowed(crate::jid::HIDDEN_USER_SERVER),
            _ => Cow::Borrowed(crate::jid::HOSTED_SERVER),
        };

        Ok(JidRef {
            user,
            server,
            agent,
            device,
            integrator: 0,
        })
    }

    fn read_interop_jid(&mut self) -> Result<JidRef<'a>> {
        let user = self
            .read_value_as_string()?
            .ok_or(BinaryError::InvalidNode)?;
        let device = self.read_u16_be()?;
        let integrator = self.read_u16_be()?;
        let server = self.read_value_as_string()?.unwrap_or(Cow::Borrowed(""));
        if server != crate::jid::INTEROP_SERVER {
            return Err(BinaryError::InvalidNode);
        }
        Ok(JidRef {
            user,
            server,
            device,
            integrator,
            agent: 0,
        })
    }

    fn read_fb_jid(&mut self) -> Result<JidRef<'a>> {
        let user = self
            .read_value_as_string()?
            .ok_or(BinaryError::InvalidNode)?;
        let device = self.read_u16_be()?;
        let server = self.read_value_as_string()?.unwrap_or(Cow::Borrowed(""));
        if server != crate::jid::MESSENGER_SERVER {
            return Err(BinaryError::InvalidNode);
        }
        Ok(JidRef {
            user,
            server,
            device,
            agent: 0,
            integrator: 0,
        })
    }

    fn read_value_as_string(&mut self) -> Result<Option<Cow<'a, str>>> {
        let tag = self.read_u8()?;
        match tag {
            token::LIST_EMPTY => Ok(None),
            token::BINARY_8 => {
                let size = self.read_u8()? as usize;
                self.read_string(size).map(Some)
            }
            token::BINARY_20 => {
                let size = self.read_u20_be()? as usize;
                self.read_string(size).map(Some)
            }
            token::BINARY_32 => {
                let size = self.read_u32_be()? as usize;
                self.read_string(size).map(Some)
            }
            token::JID_PAIR => self
                .read_jid_pair()
                .map(|j| Some(Cow::Owned(j.to_string()))),
            token::AD_JID => self.read_ad_jid().map(|j| Some(Cow::Owned(j.to_string()))),
            token::INTEROP_JID => self
                .read_interop_jid()
                .map(|j| Some(Cow::Owned(j.to_string()))),
            token::FB_JID => self.read_fb_jid().map(|j| Some(Cow::Owned(j.to_string()))),
            token::NIBBLE_8 | token::HEX_8 => self.read_packed(tag).map(|s| Some(Cow::Owned(s))),
            tag @ token::DICTIONARY_0..=token::DICTIONARY_3 => {
                let index = self.read_u8()?;
                token::get_double_token(tag - token::DICTIONARY_0, index)
                    .map(|s| Some(Cow::Borrowed(s)))
                    .ok_or(BinaryError::InvalidToken(tag))
            }
            _ => token::get_single_token(tag)
                .map(|s| Some(Cow::Borrowed(s)))
                .ok_or(BinaryError::InvalidToken(tag)),
        }
    }

    /// Read a value that can be either a string or a JID.
    /// This avoids string allocation for JID tokens by returning the JidRef directly.
    fn read_value(&mut self) -> Result<Option<ValueRef<'a>>> {
        let tag = self.read_u8()?;
        match tag {
            token::LIST_EMPTY => Ok(None),
            token::BINARY_8 => {
                let size = self.read_u8()? as usize;
                self.read_string(size).map(|s| Some(ValueRef::String(s)))
            }
            token::BINARY_20 => {
                let size = self.read_u20_be()? as usize;
                self.read_string(size).map(|s| Some(ValueRef::String(s)))
            }
            token::BINARY_32 => {
                let size = self.read_u32_be()? as usize;
                self.read_string(size).map(|s| Some(ValueRef::String(s)))
            }
            // JID tokens - return JidRef directly without string allocation
            token::JID_PAIR => self.read_jid_pair().map(|j| Some(ValueRef::Jid(j))),
            token::AD_JID => self.read_ad_jid().map(|j| Some(ValueRef::Jid(j))),
            token::INTEROP_JID => self.read_interop_jid().map(|j| Some(ValueRef::Jid(j))),
            token::FB_JID => self.read_fb_jid().map(|j| Some(ValueRef::Jid(j))),
            token::NIBBLE_8 | token::HEX_8 => self
                .read_packed(tag)
                .map(|s| Some(ValueRef::String(Cow::Owned(s)))),
            tag @ token::DICTIONARY_0..=token::DICTIONARY_3 => {
                let index = self.read_u8()?;
                token::get_double_token(tag - token::DICTIONARY_0, index)
                    .map(|s| Some(ValueRef::String(Cow::Borrowed(s))))
                    .ok_or(BinaryError::InvalidToken(tag))
            }
            _ => token::get_single_token(tag)
                .map(|s| Some(ValueRef::String(Cow::Borrowed(s))))
                .ok_or(BinaryError::InvalidToken(tag)),
        }
    }

    fn read_packed(&mut self, tag: u8) -> Result<String> {
        let packed_len_byte = self.read_u8()?;
        let is_half_byte = (packed_len_byte & 0x80) != 0;
        let len = (packed_len_byte & 0x7F) as usize;

        if len == 0 {
            return Ok(String::new());
        }

        let raw_len = if is_half_byte { (len * 2) - 1 } else { len * 2 };
        let packed_data = self.read_bytes(len)?;
        let mut unpacked_bytes = Vec::with_capacity(raw_len);

        const NIBBLE_LOOKUP: [u8; 16] = *b"0123456789-.\x00\x00\x00\x00";
        const HEX_LOOKUP: [u8; 16] = *b"0123456789ABCDEF";
        let lookup_table = Simd::from_array(if tag == token::NIBBLE_8 {
            NIBBLE_LOOKUP
        } else {
            HEX_LOOKUP
        });
        let low_mask = Simd::splat(0x0F);

        let (chunks, remainder) = packed_data.as_chunks::<16>();
        for chunk in chunks {
            let data = u8x16::from_array(*chunk);

            let high_nibbles = (data >> 4) & low_mask;
            let low_nibbles = data & low_mask;

            if tag == token::NIBBLE_8 {
                let le11 = Simd::splat(11);
                let f15 = Simd::splat(15);
                let hi_valid = high_nibbles.simd_le(le11) | high_nibbles.simd_eq(f15);
                let lo_valid = low_nibbles.simd_le(le11) | low_nibbles.simd_eq(f15);
                if !(hi_valid & lo_valid).all() {
                    for byte in *chunk {
                        let high = (byte & 0xF0) >> 4;
                        let low = byte & 0x0F;
                        Self::unpack_byte(tag, high)?;
                        Self::unpack_byte(tag, low)?;
                    }
                    unreachable!("SIMD validation should match scalar validation");
                }
            }

            let high_chars = lookup_table.swizzle_dyn(high_nibbles);
            let low_chars = lookup_table.swizzle_dyn(low_nibbles);

            let (lo, hi) = Simd::interleave(high_chars, low_chars);
            unpacked_bytes.extend_from_slice(lo.as_array());
            unpacked_bytes.extend_from_slice(hi.as_array());
        }

        for &byte in remainder {
            let high = (byte & 0xF0) >> 4;
            let low = byte & 0x0F;
            unpacked_bytes.push(Self::unpack_byte(tag, high)? as u8);
            unpacked_bytes.push(Self::unpack_byte(tag, low)? as u8);
        }

        if is_half_byte {
            unpacked_bytes.pop();
        }

        String::from_utf8(unpacked_bytes).map_err(|e| BinaryError::InvalidUtf8(e.utf8_error()))
    }

    fn unpack_byte(tag: u8, value: u8) -> Result<char> {
        match tag {
            token::NIBBLE_8 => match value {
                0..=9 => Ok((b'0' + value) as char),
                10 => Ok('-'),
                11 => Ok('.'),
                15 => Ok('\x00'),
                _ => Err(BinaryError::InvalidToken(value)),
            },
            token::HEX_8 => match value {
                0..=9 => Ok((b'0' + value) as char),
                10..=15 => Ok((b'A' + value - 10) as char),
                _ => Err(BinaryError::InvalidToken(value)),
            },
            _ => Err(BinaryError::InvalidToken(tag)),
        }
    }

    fn read_attributes(&mut self, size: usize) -> Result<AttrsRef<'a>> {
        let mut attrs = AttrsRef::with_capacity(size);
        for _ in 0..size {
            let key = self
                .read_value_as_string()?
                .ok_or(BinaryError::NonStringKey)?;
            // Use read_value to get ValueRef - avoids string allocation for JIDs
            let value = self
                .read_value()?
                .unwrap_or(ValueRef::String(Cow::Borrowed("")));
            attrs.push((key, value));
        }
        Ok(attrs)
    }

    fn read_content(&mut self) -> Result<Option<NodeContentRef<'a>>> {
        let tag = self.read_u8()?;
        match tag {
            token::LIST_EMPTY => Ok(None),

            token::LIST_8 | token::LIST_16 => {
                let size = self.read_list_size(tag)?;
                let mut nodes = NodeVec::with_capacity(size);
                for _ in 0..size {
                    nodes.push(self.read_node_ref()?);
                }
                Ok(Some(NodeContentRef::Nodes(Box::new(nodes))))
            }

            token::BINARY_8 => {
                let len = self.read_u8()? as usize;
                let bytes = self.read_bytes(len)?;
                Ok(Some(NodeContentRef::Bytes(Cow::Borrowed(bytes))))
            }
            token::BINARY_20 => {
                let len = self.read_u20_be()? as usize;
                let bytes = self.read_bytes(len)?;
                Ok(Some(NodeContentRef::Bytes(Cow::Borrowed(bytes))))
            }
            token::BINARY_32 => {
                let len = self.read_u32_be()? as usize;
                let bytes = self.read_bytes(len)?;
                Ok(Some(NodeContentRef::Bytes(Cow::Borrowed(bytes))))
            }

            _ => {
                self.position -= 1;
                let string_content = self.read_value_as_string()?;

                match string_content {
                    Some(s) => Ok(Some(NodeContentRef::String(s))),
                    None => Ok(None),
                }
            }
        }
    }

    pub(crate) fn read_node_ref(&mut self) -> Result<NodeRef<'a>> {
        let tag = self.read_u8()?;
        let list_size = self.read_list_size(tag)?;
        if list_size == 0 {
            return Err(BinaryError::InvalidNode);
        }

        let tag = self
            .read_value_as_string()?
            .ok_or(BinaryError::InvalidNode)?;

        let attr_count = (list_size - 1) / 2;
        let has_content = list_size.is_multiple_of(2);

        let attrs = self.read_attributes(attr_count)?;
        let content = if has_content {
            self.read_content()?.map(Box::new)
        } else {
            None
        };

        Ok(NodeRef {
            tag,
            attrs,
            content,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::node::Node;

    type TestResult = crate::error::Result<()>;

    #[test]
    fn test_decode_node() -> TestResult {
        let node = Node::new(
            "message",
            indexmap::IndexMap::new(),
            Some(crate::node::NodeContent::String("receipt".to_string())),
        );

        let mut buffer = Vec::new();
        {
            let mut encoder = crate::encoder::Encoder::new(std::io::Cursor::new(&mut buffer))?;
            encoder.write_node(&node)?;
        }

        let mut decoder = Decoder::new(&buffer[1..]);
        let decoded = decoder.read_node_ref().unwrap();

        assert_eq!(decoded.tag, "message");
        assert!(decoded.attrs.is_empty());
        match &decoded.content {
            Some(content) => match &**content {
                crate::node::NodeContentRef::String(s) => assert_eq!(s, "receipt"),
                _ => panic!("Expected string content"),
            },
            None => panic!("Expected content"),
        }
        Ok(())
    }

    #[test]
    fn test_decode_nibble_packing() -> TestResult {
        let test_str = "-.0123456789";
        let node = Node::new(
            "test",
            indexmap::IndexMap::new(),
            Some(crate::node::NodeContent::String(test_str.to_string())),
        );

        let mut buffer = Vec::new();
        {
            let mut encoder = crate::encoder::Encoder::new(std::io::Cursor::new(&mut buffer))?;
            encoder.write_node(&node)?;
        }

        let mut decoder = Decoder::new(&buffer[1..]);
        let decoded = decoder.read_node_ref().unwrap();

        assert_eq!(decoded.tag, "test");
        assert!(decoded.attrs.is_empty());
        match &decoded.content {
            Some(content) => match &**content {
                crate::node::NodeContentRef::String(s) => assert_eq!(s, test_str),
                _ => panic!("Expected string content"),
            },
            None => panic!("Expected content"),
        }
        Ok(())
    }

    #[test]
    fn test_invalid_nibble_rejection() {
        let invalid_data = vec![1, 0xC0];

        let mut decoder = Decoder::new(&invalid_data);
        let result = decoder.read_packed(token::NIBBLE_8);
        assert!(
            result.is_err(),
            "Expected error for invalid nibble 12, got: {:?}",
            result
        );

        if let Err(BinaryError::InvalidToken(invalid_nibble)) = result {
            assert_eq!(invalid_nibble, 12, "Expected invalid nibble 12");
        } else {
            panic!("Expected InvalidToken error, got: {:?}", result);
        }
    }

    /// Test empty input returns appropriate error
    #[test]
    fn test_empty_input() {
        let mut decoder = Decoder::new(&[]);
        let result = decoder.read_node_ref();
        assert!(result.is_err());
    }

    /// Test truncated u16 read
    #[test]
    fn test_truncated_u16() {
        // Only one byte when u16 expected
        let data = vec![0x42];
        let mut decoder = Decoder::new(&data);
        let result = decoder.read_u16_be();
        assert!(result.is_err());
        if let Err(BinaryError::UnexpectedEof) = result {
            // Expected
        } else {
            panic!("Expected UnexpectedEof, got: {:?}", result);
        }
    }

    /// Test truncated u20 read
    #[test]
    fn test_truncated_u20() {
        // Only two bytes when u20 (3 bytes) expected
        let data = vec![0x42, 0x43];
        let mut decoder = Decoder::new(&data);
        let result = decoder.read_u20_be();
        assert!(result.is_err());
    }

    /// Test truncated u32 read
    #[test]
    fn test_truncated_u32() {
        // Only three bytes when u32 expected
        let data = vec![0x42, 0x43, 0x44];
        let mut decoder = Decoder::new(&data);
        let result = decoder.read_u32_be();
        assert!(result.is_err());
    }

    /// Test BINARY_8 with length larger than remaining buffer
    #[test]
    fn test_binary8_length_exceeds_buffer() {
        // BINARY_8 token, length 100, but only 5 bytes of data
        let data = vec![token::BINARY_8, 100, 1, 2, 3, 4, 5];
        let mut decoder = Decoder::new(&data);
        let result = decoder.read_value_as_string();
        assert!(result.is_err());
    }

    /// Test BINARY_20 with length larger than remaining buffer
    #[test]
    fn test_binary20_length_exceeds_buffer() {
        // BINARY_20 token, length encoded as 256, but only a few bytes of data
        let data = vec![token::BINARY_20, 0x00, 0x01, 0x00, 1, 2, 3]; // length = 256
        let mut decoder = Decoder::new(&data);
        let result = decoder.read_value_as_string();
        assert!(result.is_err());
    }

    /// Test LIST_8 with size larger than remaining data
    #[test]
    fn test_list8_size_exceeds_data() {
        // LIST_8 token, size 10, but not enough data for 10 nodes
        let data = vec![token::LIST_8, 10, 1]; // Only 1 byte of data for nodes
        let mut decoder = Decoder::new(&data);
        let result = decoder.read_node_ref();
        assert!(result.is_err());
    }

    /// Test invalid token value
    #[test]
    fn test_invalid_token() {
        // Use a token value that's reserved and not valid as a string token
        // e.g., AD_JID (247) followed by insufficient data
        let data = vec![token::AD_JID]; // No data following
        let mut decoder = Decoder::new(&data);
        let result = decoder.read_value_as_string();
        assert!(result.is_err());
    }

    /// Test read_bytes with exact length
    #[test]
    fn test_read_bytes_exact_length() {
        let data = vec![1, 2, 3, 4, 5];
        let mut decoder = Decoder::new(&data);
        let bytes = decoder.read_bytes(5).unwrap();
        assert_eq!(bytes, &[1, 2, 3, 4, 5]);
        assert!(decoder.is_finished());
    }

    /// Test read_bytes exceeding length
    #[test]
    fn test_read_bytes_exceeding_length() {
        let data = vec![1, 2, 3];
        let mut decoder = Decoder::new(&data);
        let result = decoder.read_bytes(5);
        assert!(result.is_err());
    }

    /// Test u20 encoding/decoding values
    #[test]
    fn test_u20_encoding() {
        // Test value 0
        let data = vec![0x00, 0x00, 0x00];
        let mut decoder = Decoder::new(&data);
        assert_eq!(decoder.read_u20_be().unwrap(), 0);

        // Test value 256 (0x100)
        let data = vec![0x00, 0x01, 0x00];
        let mut decoder = Decoder::new(&data);
        assert_eq!(decoder.read_u20_be().unwrap(), 256);

        // Test value 65536 (0x10000)
        let data = vec![0x01, 0x00, 0x00];
        let mut decoder = Decoder::new(&data);
        assert_eq!(decoder.read_u20_be().unwrap(), 65536);

        // Test max u20 value (0xFFFFF = 1048575)
        let data = vec![0x0F, 0xFF, 0xFF];
        let mut decoder = Decoder::new(&data);
        assert_eq!(decoder.read_u20_be().unwrap(), 1048575);
    }

    /// Test bytes_left tracking
    #[test]
    fn test_bytes_left() {
        let data = vec![1, 2, 3, 4, 5];
        let mut decoder = Decoder::new(&data);

        assert_eq!(decoder.bytes_left(), 5);
        decoder.read_u8().unwrap();
        assert_eq!(decoder.bytes_left(), 4);
        decoder.read_u8().unwrap();
        assert_eq!(decoder.bytes_left(), 3);
        decoder.read_bytes(3).unwrap();
        assert_eq!(decoder.bytes_left(), 0);
        assert!(decoder.is_finished());
    }

    /// Test hex packed string decoding
    #[test]
    fn test_hex_packed_decoding() {
        // Encode "ABCDEF" as hex packed
        // Each byte packs two hex digits
        // A=10, B=11, C=12, D=13, E=14, F=15
        let packed_data = vec![
            3,    // length = 3 bytes = 6 characters
            0xAB, // AB
            0xCD, // CD
            0xEF, // EF
        ];

        let mut decoder = Decoder::new(&packed_data);
        let result = decoder.read_packed(token::HEX_8).unwrap();
        assert_eq!(result, "ABCDEF");
    }

    /// Test nibble packed string with odd length
    #[test]
    fn test_nibble_packed_odd_length() {
        // Encode "123" as nibble packed (odd length = 3)
        // 1=1, 2=2, 3=3, pad=15
        let packed_data = vec![
            0x82, // length = 2 bytes, high bit set for odd
            0x12, // 12
            0x3F, // 3 + pad (15)
        ];

        let mut decoder = Decoder::new(&packed_data);
        let result = decoder.read_packed(token::NIBBLE_8).unwrap();
        assert_eq!(result, "123");
    }

    /// Test empty packed string
    #[test]
    fn test_empty_packed_string() {
        let packed_data = vec![0]; // length = 0

        let mut decoder = Decoder::new(&packed_data);
        let result = decoder.read_packed(token::NIBBLE_8).unwrap();
        assert_eq!(result, "");
    }

    /// Test invalid nibble value 12 (only 0-11, 15 are valid)
    #[test]
    fn test_invalid_nibble_value_12() {
        // 12 (0xC) is not a valid nibble
        let packed_data = vec![1, 0xC0]; // first nibble is 12

        let mut decoder = Decoder::new(&packed_data);
        let result = decoder.read_packed(token::NIBBLE_8);
        assert!(result.is_err());
    }

    /// Test invalid nibble value 13
    #[test]
    fn test_invalid_nibble_value_13() {
        let packed_data = vec![1, 0xD0]; // first nibble is 13

        let mut decoder = Decoder::new(&packed_data);
        let result = decoder.read_packed(token::NIBBLE_8);
        assert!(result.is_err());
    }

    /// Test invalid nibble value 14
    #[test]
    fn test_invalid_nibble_value_14() {
        let packed_data = vec![1, 0xE0]; // first nibble is 14

        let mut decoder = Decoder::new(&packed_data);
        let result = decoder.read_packed(token::NIBBLE_8);
        assert!(result.is_err());
    }

    /// Test deeply nested nodes (recursion safety)
    #[test]
    fn test_nested_nodes() -> TestResult {
        // Create a 50-level deep node structure
        let mut current = Node::new("leaf", indexmap::IndexMap::new(), None);

        for i in 0..50 {
            let tag = format!("level{}", i);
            current = Node::new(
                &tag,
                indexmap::IndexMap::new(),
                Some(crate::node::NodeContent::Nodes(vec![current])),
            );
        }

        let mut buffer = Vec::new();
        {
            let mut encoder = crate::encoder::Encoder::new(std::io::Cursor::new(&mut buffer))?;
            encoder.write_node(&current)?;
        }

        let mut decoder = Decoder::new(&buffer[1..]);
        let decoded = decoder.read_node_ref()?;

        // Verify top level tag
        assert_eq!(decoded.tag, "level49");
        Ok(())
    }
}
