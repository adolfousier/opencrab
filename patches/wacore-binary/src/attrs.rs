use std::borrow::Cow;
use std::str::FromStr;

use crate::error::{BinaryError, Result};
use crate::jid::Jid;
use crate::node::{Attrs, Node, NodeRef, ValueRef};

pub struct AttrParser<'a> {
    pub attrs: &'a Attrs,
    pub errors: Vec<BinaryError>,
}

pub struct AttrParserRef<'a> {
    pub attrs: &'a [(Cow<'a, str>, ValueRef<'a>)],
    pub errors: Vec<BinaryError>,
}

impl<'a> AttrParserRef<'a> {
    pub fn new(node: &'a NodeRef<'a>) -> Self {
        Self {
            attrs: node.attrs.as_slice(),
            errors: Vec::new(),
        }
    }

    pub fn ok(&self) -> bool {
        self.errors.is_empty()
    }

    pub fn finish(&self) -> Result<()> {
        if self.ok() {
            Ok(())
        } else {
            Err(BinaryError::AttrList(self.errors.clone()))
        }
    }

    fn get_raw(&mut self, key: &str, require: bool) -> Option<&'a ValueRef<'a>> {
        let val = self
            .attrs
            .iter()
            .find(|(k, _)| k.as_ref() == key)
            .map(|(_, v)| v);

        if require && val.is_none() {
            self.errors.push(BinaryError::AttrParse(format!(
                "Required attribute '{key}' not found"
            )));
        }

        val
    }

    /// Get string from the value.
    /// For JID values, this returns None - use optional_jid instead.
    pub fn optional_string(&mut self, key: &str) -> Option<&'a str> {
        self.get_raw(key, false).and_then(|v| v.as_str())
    }

    pub fn string(&mut self, key: &str) -> String {
        self.get_raw(key, true)
            .map(|v| v.to_string_cow().into_owned())
            .unwrap_or_default()
    }

    /// Get JID from the value.
    /// If the value is a JidRef, returns it directly without parsing (zero allocation).
    /// If the value is a string, parses it as a JID.
    pub fn optional_jid(&mut self, key: &str) -> Option<Jid> {
        self.get_raw(key, false).and_then(|v| match v.to_jid() {
            Some(jid) => Some(jid),
            None => {
                // to_jid() only returns None if it's a String that failed to parse
                if let ValueRef::String(s) = v {
                    self.errors
                        .push(BinaryError::AttrParse(format!("Invalid JID: {s}")));
                }
                None
            }
        })
    }

    pub fn jid(&mut self, key: &str) -> Jid {
        self.get_raw(key, true);
        self.optional_jid(key).unwrap_or_default()
    }

    pub fn non_ad_jid(&mut self, key: &str) -> Jid {
        self.jid(key).to_non_ad()
    }

    fn get_string_value(&mut self, key: &str, require: bool) -> Option<Cow<'a, str>> {
        self.get_raw(key, require).map(|v| v.to_string_cow())
    }

    fn get_bool(&mut self, key: &str, require: bool) -> Option<bool> {
        self.get_string_value(key, require)
            .and_then(|s| match s.parse::<bool>() {
                Ok(val) => Some(val),
                Err(e) => {
                    self.errors.push(BinaryError::AttrParse(format!(
                        "Failed to parse bool from '{s}' for key '{key}': {e}"
                    )));
                    None
                }
            })
    }

    pub fn optional_bool(&mut self, key: &str) -> bool {
        self.get_bool(key, false).unwrap_or(false)
    }

    pub fn bool(&mut self, key: &str) -> bool {
        self.get_bool(key, true).unwrap_or(false)
    }

    pub fn optional_u64(&mut self, key: &str) -> Option<u64> {
        self.get_string_value(key, false)
            .and_then(|s| match s.parse::<u64>() {
                Ok(val) => Some(val),
                Err(e) => {
                    self.errors.push(BinaryError::AttrParse(format!(
                        "Failed to parse u64 from '{s}' for key '{key}': {e}"
                    )));
                    None
                }
            })
    }

    pub fn unix_time(&mut self, key: &str) -> i64 {
        self.get_raw(key, true);
        self.optional_unix_time(key).unwrap_or_default()
    }

    pub fn optional_unix_time(&mut self, key: &str) -> Option<i64> {
        self.get_i64(key, false)
    }

    pub fn unix_milli(&mut self, key: &str) -> i64 {
        self.get_raw(key, true);
        self.optional_unix_milli(key).unwrap_or_default()
    }

    pub fn optional_unix_milli(&mut self, key: &str) -> Option<i64> {
        self.get_i64(key, false)
    }

    fn get_i64(&mut self, key: &str, require: bool) -> Option<i64> {
        self.get_string_value(key, require)
            .and_then(|s| match s.parse::<i64>() {
                Ok(val) => Some(val),
                Err(e) => {
                    self.errors.push(BinaryError::AttrParse(format!(
                        "Failed to parse i64 from '{s}' for key '{key}': {e}"
                    )));
                    None
                }
            })
    }
}

impl<'a> AttrParser<'a> {
    pub fn new(node: &'a Node) -> Self {
        Self {
            attrs: &node.attrs,
            errors: Vec::new(),
        }
    }

    pub fn ok(&self) -> bool {
        self.errors.is_empty()
    }

    pub fn finish(&self) -> Result<()> {
        if self.ok() {
            Ok(())
        } else {
            Err(BinaryError::AttrList(self.errors.clone()))
        }
    }

    fn get_raw(&mut self, key: &str, require: bool) -> Option<&'a String> {
        let val = self.attrs.get(key);
        if require && val.is_none() {
            self.errors.push(BinaryError::AttrParse(format!(
                "Required attribute '{key}' not found"
            )));
        }
        val
    }

    // --- String ---
    pub fn optional_string(&mut self, key: &str) -> Option<&'a str> {
        self.get_raw(key, false).map(|s| s.as_str())
    }

    pub fn string(&mut self, key: &str) -> String {
        self.get_raw(key, true).cloned().unwrap_or_default()
    }

    // --- JID ---
    pub fn optional_jid(&mut self, key: &str) -> Option<Jid> {
        self.get_raw(key, false)
            .and_then(|s| match Jid::from_str(s) {
                Ok(jid) => Some(jid),
                Err(e) => {
                    self.errors.push(BinaryError::from(e));
                    None
                }
            })
    }

    pub fn jid(&mut self, key: &str) -> Jid {
        self.get_raw(key, true); // Push "not found" error if needed.
        self.optional_jid(key).unwrap_or_default()
    }

    pub fn non_ad_jid(&mut self, key: &str) -> Jid {
        self.jid(key).to_non_ad()
    }

    // --- Boolean ---
    fn get_bool(&mut self, key: &str, require: bool) -> Option<bool> {
        self.get_raw(key, require)
            .and_then(|s| match s.parse::<bool>() {
                Ok(val) => Some(val),
                Err(e) => {
                    self.errors.push(BinaryError::AttrParse(format!(
                        "Failed to parse bool from '{s}' for key '{key}': {e}"
                    )));
                    None
                }
            })
    }

    pub fn optional_bool(&mut self, key: &str) -> bool {
        self.get_bool(key, false).unwrap_or(false)
    }

    pub fn bool(&mut self, key: &str) -> bool {
        self.get_bool(key, true).unwrap_or(false)
    }

    // --- u64 ---
    pub fn optional_u64(&mut self, key: &str) -> Option<u64> {
        self.get_raw(key, false)
            .and_then(|s| match s.parse::<u64>() {
                Ok(val) => Some(val),
                Err(e) => {
                    self.errors.push(BinaryError::AttrParse(format!(
                        "Failed to parse u64 from '{s}' for key '{key}': {e}"
                    )));
                    None
                }
            })
    }

    pub fn unix_time(&mut self, key: &str) -> i64 {
        self.get_raw(key, true);
        self.optional_unix_time(key).unwrap_or_default()
    }

    pub fn optional_unix_time(&mut self, key: &str) -> Option<i64> {
        self.get_i64(key, false)
    }

    pub fn unix_milli(&mut self, key: &str) -> i64 {
        self.get_raw(key, true);
        self.optional_unix_milli(key).unwrap_or_default()
    }

    pub fn optional_unix_milli(&mut self, key: &str) -> Option<i64> {
        self.get_i64(key, false)
    }

    fn get_i64(&mut self, key: &str, require: bool) -> Option<i64> {
        self.get_raw(key, require)
            .and_then(|s| match s.parse::<i64>() {
                Ok(val) => Some(val),
                Err(e) => {
                    self.errors.push(BinaryError::AttrParse(format!(
                        "Failed to parse i64 from '{s}' for key '{key}': {e}"
                    )));
                    None
                }
            })
    }
}
