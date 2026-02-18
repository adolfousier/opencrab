use crate::node::{Node, NodeContent};
use indexmap::IndexMap;

#[derive(Debug, Default)]
pub struct NodeBuilder {
    tag: String,
    attrs: IndexMap<String, String>,
    content: Option<NodeContent>,
}

impl NodeBuilder {
    pub fn new(tag: impl Into<String>) -> Self {
        Self {
            tag: tag.into(),
            ..Default::default()
        }
    }

    pub fn attr(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.attrs.insert(key.into(), value.into());
        self
    }

    pub fn attrs<I, K, V>(mut self, attrs: I) -> Self
    where
        I: IntoIterator<Item = (K, V)>,
        K: Into<String>,
        V: Into<String>,
    {
        for (key, value) in attrs.into_iter() {
            self.attrs.insert(key.into(), value.into());
        }
        self
    }

    pub fn children(mut self, children: impl IntoIterator<Item = Node>) -> Self {
        self.content = Some(NodeContent::Nodes(children.into_iter().collect()));
        self
    }

    pub fn bytes(mut self, bytes: impl Into<Vec<u8>>) -> Self {
        self.content = Some(NodeContent::Bytes(bytes.into()));
        self
    }

    pub fn string_content(mut self, s: impl Into<String>) -> Self {
        self.content = Some(NodeContent::String(s.into()));
        self
    }

    pub fn build(self) -> Node {
        Node {
            tag: self.tag,
            attrs: self.attrs,
            content: self.content,
        }
    }

    pub fn apply_content(mut self, content: Option<NodeContent>) -> Self {
        self.content = content;
        self
    }
}
