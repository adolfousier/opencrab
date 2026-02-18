use std::io::Write;

use crate::{BinaryError, Node, NodeRef, Result, decoder::Decoder, encoder::Encoder};

pub fn unmarshal_ref(data: &[u8]) -> Result<NodeRef<'_>> {
    let mut decoder = Decoder::new(data);
    let node = decoder.read_node_ref()?;

    if decoder.is_finished() {
        Ok(node)
    } else {
        Err(BinaryError::LeftoverData(decoder.bytes_left()))
    }
}

pub fn marshal_to(node: &Node, writer: &mut impl Write) -> Result<()> {
    let mut encoder = Encoder::new(writer)?;
    encoder.write_node(node)?;
    Ok(())
}

pub fn marshal(node: &Node) -> Result<Vec<u8>> {
    let mut payload = Vec::with_capacity(1024);
    marshal_to(node, &mut payload)?;
    Ok(payload)
}

/// Zero-copy serialization of a `NodeRef` directly into a writer.
/// This avoids the allocation overhead of converting to an owned `Node` first.
pub fn marshal_ref_to(node: &NodeRef<'_>, writer: &mut impl Write) -> Result<()> {
    let mut encoder = Encoder::new(writer)?;
    encoder.write_node_ref(node)?;
    Ok(())
}

/// Zero-copy serialization of a `NodeRef` to a new `Vec<u8>`.
/// Prefer `marshal_ref_to` with a reusable buffer for best performance.
pub fn marshal_ref(node: &NodeRef<'_>) -> Result<Vec<u8>> {
    let mut payload = Vec::with_capacity(1024);
    marshal_ref_to(node, &mut payload)?;
    Ok(payload)
}
