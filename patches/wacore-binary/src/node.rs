use crate::attrs::{AttrParser, AttrParserRef};
use crate::jid::{Jid, JidRef};
use indexmap::IndexMap;
use std::borrow::Cow;

pub type Attrs = IndexMap<String, String>;
pub type AttrsRef<'a> = Vec<(Cow<'a, str>, ValueRef<'a>)>;

/// A decoded attribute value that can be either a string or a structured JID.
/// This avoids string allocation when decoding JID tokens - the JidRef is returned
/// directly and only converted to a string when actually needed.
#[derive(Debug, Clone, PartialEq)]
pub enum ValueRef<'a> {
    String(Cow<'a, str>),
    Jid(JidRef<'a>),
}

impl<'a> ValueRef<'a> {
    /// Get the value as a string slice, if it's a string variant.
    pub fn as_str(&self) -> Option<&str> {
        match self {
            ValueRef::String(s) => Some(s.as_ref()),
            ValueRef::Jid(_) => None,
        }
    }

    /// Get the value as a JidRef, if it's a JID variant.
    pub fn as_jid(&self) -> Option<&JidRef<'a>> {
        match self {
            ValueRef::Jid(j) => Some(j),
            ValueRef::String(_) => None,
        }
    }

    /// Convert to an owned Jid, parsing from string if necessary.
    pub fn to_jid(&self) -> Option<Jid> {
        match self {
            ValueRef::Jid(j) => Some(j.to_owned()),
            ValueRef::String(s) => Jid::from_str(s.as_ref()).ok(),
        }
    }

    /// Convert to a string, formatting the JID if necessary.
    /// Returns a Cow to avoid allocation when the value is already a string.
    pub fn to_string_cow(&self) -> Cow<'a, str> {
        match self {
            ValueRef::String(s) => s.clone(),
            ValueRef::Jid(j) => Cow::Owned(j.to_string()),
        }
    }
}

use std::fmt;
use std::str::FromStr;

impl<'a> fmt::Display for ValueRef<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ValueRef::String(s) => write!(f, "{}", s),
            ValueRef::Jid(j) => write!(f, "{}", j),
        }
    }
}

pub type NodeVec<'a> = Vec<NodeRef<'a>>;

#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, PartialEq)]
pub enum NodeContent {
    Bytes(Vec<u8>),
    String(String),
    Nodes(Vec<Node>),
}

#[derive(Debug, Clone, PartialEq)]
pub enum NodeContentRef<'a> {
    Bytes(Cow<'a, [u8]>),
    String(Cow<'a, str>),
    Nodes(Box<NodeVec<'a>>),
}

impl NodeContent {
    /// Convert an owned NodeContent to a borrowed NodeContentRef.
    pub fn as_content_ref(&self) -> NodeContentRef<'_> {
        match self {
            NodeContent::Bytes(b) => NodeContentRef::Bytes(Cow::Borrowed(b)),
            NodeContent::String(s) => NodeContentRef::String(Cow::Borrowed(s)),
            NodeContent::Nodes(nodes) => {
                NodeContentRef::Nodes(Box::new(nodes.iter().map(|n| n.as_node_ref()).collect()))
            }
        }
    }
}

#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Node {
    pub tag: String,
    pub attrs: Attrs,
    pub content: Option<NodeContent>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct NodeRef<'a> {
    pub tag: Cow<'a, str>,
    pub attrs: AttrsRef<'a>,
    pub content: Option<Box<NodeContentRef<'a>>>,
}

impl Node {
    pub fn new(tag: &str, attrs: Attrs, content: Option<NodeContent>) -> Self {
        Self {
            tag: tag.to_string(),
            attrs,
            content,
        }
    }

    /// Convert an owned Node to a borrowed NodeRef.
    /// The returned NodeRef borrows from self.
    pub fn as_node_ref(&self) -> NodeRef<'_> {
        NodeRef {
            tag: Cow::Borrowed(&self.tag),
            attrs: self
                .attrs
                .iter()
                .map(|(k, v)| {
                    (
                        Cow::Borrowed(k.as_str()),
                        ValueRef::String(Cow::Borrowed(v.as_str())),
                    )
                })
                .collect(),
            content: self.content.as_ref().map(|c| Box::new(c.as_content_ref())),
        }
    }

    pub fn children(&self) -> Option<&[Node]> {
        match &self.content {
            Some(NodeContent::Nodes(nodes)) => Some(nodes),
            _ => None,
        }
    }

    pub fn attrs(&self) -> AttrParser<'_> {
        AttrParser::new(self)
    }

    pub fn get_optional_child_by_tag<'a>(&'a self, tags: &[&str]) -> Option<&'a Node> {
        let mut current_node = self;
        for &tag in tags {
            if let Some(children) = current_node.children() {
                if let Some(found) = children.iter().find(|c| c.tag == tag) {
                    current_node = found;
                } else {
                    return None;
                }
            } else {
                return None;
            }
        }
        Some(current_node)
    }

    pub fn get_children_by_tag(&self, tag: &str) -> Vec<&Node> {
        if let Some(children) = self.children() {
            children.iter().filter(|c| c.tag == tag).collect()
        } else {
            Vec::new()
        }
    }

    pub fn get_optional_child(&self, tag: &str) -> Option<&Node> {
        self.children()
            .and_then(|nodes| nodes.iter().find(|node| node.tag == tag))
    }
}

impl<'a> NodeRef<'a> {
    pub fn new(
        tag: Cow<'a, str>,
        attrs: AttrsRef<'a>,
        content: Option<NodeContentRef<'a>>,
    ) -> Self {
        Self {
            tag,
            attrs,
            content: content.map(Box::new),
        }
    }

    pub fn attr_parser(&'a self) -> AttrParserRef<'a> {
        AttrParserRef::new(self)
    }

    pub fn children(&self) -> Option<&[NodeRef<'a>]> {
        match self.content.as_deref() {
            Some(NodeContentRef::Nodes(nodes)) => Some(nodes.as_slice()),
            _ => None,
        }
    }

    pub fn get_attr(&self, key: &str) -> Option<&ValueRef<'a>> {
        self.attrs.iter().find(|(k, _)| k == key).map(|(_, v)| v)
    }

    pub fn attrs_iter(&self) -> impl Iterator<Item = (&Cow<'a, str>, &ValueRef<'a>)> {
        self.attrs.iter().map(|(k, v)| (k, v))
    }

    pub fn get_optional_child_by_tag(&self, tags: &[&str]) -> Option<&NodeRef<'a>> {
        let mut current_node = self;
        for &tag in tags {
            if let Some(children) = current_node.children() {
                if let Some(found) = children.iter().find(|c| c.tag == tag) {
                    current_node = found;
                } else {
                    return None;
                }
            } else {
                return None;
            }
        }
        Some(current_node)
    }

    pub fn get_children_by_tag(&self, tag: &str) -> Vec<&NodeRef<'a>> {
        if let Some(children) = self.children() {
            children.iter().filter(|c| c.tag == tag).collect()
        } else {
            Vec::new()
        }
    }

    pub fn get_optional_child(&self, tag: &str) -> Option<&NodeRef<'a>> {
        self.children()
            .and_then(|nodes| nodes.iter().find(|node| node.tag == tag))
    }

    pub fn to_owned(&self) -> Node {
        Node {
            tag: self.tag.to_string(),
            attrs: self
                .attrs
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string_cow().into_owned()))
                .collect::<IndexMap<String, String>>(),
            content: self.content.as_deref().map(|c| match c {
                NodeContentRef::Bytes(b) => NodeContent::Bytes(b.to_vec()),
                NodeContentRef::String(s) => NodeContent::String(s.to_string()),
                NodeContentRef::Nodes(nodes) => {
                    NodeContent::Nodes(nodes.iter().map(|n| n.to_owned()).collect())
                }
            }),
        }
    }
}
