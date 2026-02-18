use std::borrow::Cow;
use std::fmt;
use std::str::FromStr;

/// Intermediate result from fast JID parsing.
/// This avoids allocations by returning byte indices into the original string.
#[derive(Debug, Clone, Copy)]
pub struct ParsedJidParts<'a> {
    pub user: &'a str,
    pub server: &'a str,
    pub agent: u8,
    pub device: u16,
    pub integrator: u16,
}

/// Single-pass JID parser optimized for hot paths.
/// Scans the input string once to find all relevant separators (@, :)
/// and returns slices into the original string without allocation.
///
/// Returns `None` for JIDs that need full validation (edge cases, unknown servers, etc.)
#[inline]
pub fn parse_jid_fast(s: &str) -> Option<ParsedJidParts<'_>> {
    if s.is_empty() {
        return None;
    }

    let bytes = s.as_bytes();

    // Single pass to find key separator positions
    let mut at_pos: Option<usize> = None;
    let mut colon_pos: Option<usize> = None;
    let mut last_dot_pos: Option<usize> = None;

    for (i, &b) in bytes.iter().enumerate() {
        match b {
            b'@' => {
                if at_pos.is_none() {
                    at_pos = Some(i);
                }
            }
            b':' => {
                // Only track colon in user part (before @)
                if at_pos.is_none() {
                    colon_pos = Some(i);
                }
            }
            b'.' => {
                // Only track dots in user part (before @ and before :)
                if at_pos.is_none() && colon_pos.is_none() {
                    last_dot_pos = Some(i);
                }
            }
            _ => {}
        }
    }

    // Extract at_pos as concrete value - after this point we know @ exists
    let at = match at_pos {
        Some(pos) => pos,
        None => {
            // Server-only JID - let the fallback validate it
            return None;
        }
    };

    let user_part = &s[..at];
    let server = &s[at + 1..];

    // Validate that user_part is not empty
    if user_part.is_empty() {
        return None;
    }

    // Fast path for LID JIDs - dots in user are not agent separators
    if server == HIDDEN_USER_SERVER {
        let (user, device) = match colon_pos {
            Some(pos) if pos < at => {
                let device_slice = &s[pos + 1..at];
                (&s[..pos], device_slice.parse::<u16>().unwrap_or(0))
            }
            _ => (user_part, 0),
        };
        return Some(ParsedJidParts {
            user,
            server,
            agent: 0,
            device,
            integrator: 0,
        });
    }

    // For DEFAULT_USER_SERVER (s.whatsapp.net), handle legacy dot format as device
    if server == DEFAULT_USER_SERVER {
        // Check for colon format first (modern: user:device@server)
        if let Some(pos) = colon_pos {
            let user_end = pos;
            let device_start = pos + 1;
            let device_slice = &s[device_start..at];
            let device = device_slice.parse::<u16>().unwrap_or(0);
            return Some(ParsedJidParts {
                user: &s[..user_end],
                server,
                agent: 0,
                device,
                integrator: 0,
            });
        }
        // Check for legacy dot format (legacy: user.device@server)
        if let Some(dot_pos) = last_dot_pos {
            // dot_pos is absolute position in s
            let suffix = &s[dot_pos + 1..at];
            if let Ok(device_val) = suffix.parse::<u16>() {
                return Some(ParsedJidParts {
                    user: &s[..dot_pos],
                    server,
                    agent: 0,
                    device: device_val,
                    integrator: 0,
                });
            }
        }
        // No device component
        return Some(ParsedJidParts {
            user: user_part,
            server,
            agent: 0,
            device: 0,
            integrator: 0,
        });
    }

    // Parse device from colon separator (user:device@server)
    let (user_before_colon, device) = match colon_pos {
        Some(pos) => {
            // Colon is at `pos` in the original string
            let user_end = pos;
            let device_start = pos + 1;
            let device_slice = &s[device_start..at];
            (&s[..user_end], device_slice.parse::<u16>().unwrap_or(0))
        }
        None => (user_part, 0),
    };

    // Parse agent from last dot in user part (for non-default, non-LID servers)
    let user_to_check = user_before_colon;
    let (final_user, agent) = {
        if let Some(dot_pos) = user_to_check.rfind('.') {
            let suffix = &user_to_check[dot_pos + 1..];
            if let Ok(agent_val) = suffix.parse::<u16>() {
                if agent_val <= u8::MAX as u16 {
                    (&user_to_check[..dot_pos], agent_val as u8)
                } else {
                    (user_to_check, 0)
                }
            } else {
                (user_to_check, 0)
            }
        } else {
            (user_to_check, 0)
        }
    };

    Some(ParsedJidParts {
        user: final_user,
        server,
        agent,
        device,
        integrator: 0,
    })
}

pub const DEFAULT_USER_SERVER: &str = "s.whatsapp.net";
pub const SERVER_JID: &str = "s.whatsapp.net";
pub const GROUP_SERVER: &str = "g.us";
pub const LEGACY_USER_SERVER: &str = "c.us";
pub const BROADCAST_SERVER: &str = "broadcast";
pub const HIDDEN_USER_SERVER: &str = "lid";
pub const NEWSLETTER_SERVER: &str = "newsletter";
pub const HOSTED_SERVER: &str = "hosted";
pub const HOSTED_LID_SERVER: &str = "hosted.lid";
pub const MESSENGER_SERVER: &str = "msgr";
pub const INTEROP_SERVER: &str = "interop";
pub const BOT_SERVER: &str = "bot";
pub const STATUS_BROADCAST_USER: &str = "status";

pub type MessageId = String;
pub type MessageServerId = i32;
#[derive(Debug)]
pub enum JidError {
    // REMOVE: #[error("...")]
    InvalidFormat(String),
    // REMOVE: #[error("...")]
    Parse(std::num::ParseIntError),
}

impl fmt::Display for JidError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            JidError::InvalidFormat(s) => write!(f, "Invalid JID format: {s}"),
            JidError::Parse(e) => write!(f, "Failed to parse component: {e}"),
        }
    }
}

impl std::error::Error for JidError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            JidError::Parse(e) => Some(e),
            _ => None,
        }
    }
}

// Add From impl
impl From<std::num::ParseIntError> for JidError {
    fn from(err: std::num::ParseIntError) -> Self {
        JidError::Parse(err)
    }
}

pub trait JidExt {
    fn user(&self) -> &str;
    fn server(&self) -> &str;
    fn device(&self) -> u16;
    fn integrator(&self) -> u16;

    fn is_ad(&self) -> bool {
        self.device() > 0
            && (self.server() == DEFAULT_USER_SERVER
                || self.server() == HIDDEN_USER_SERVER
                || self.server() == HOSTED_SERVER)
    }

    fn is_interop(&self) -> bool {
        self.server() == INTEROP_SERVER && self.integrator() > 0
    }

    fn is_messenger(&self) -> bool {
        self.server() == MESSENGER_SERVER && self.device() > 0
    }

    fn is_group(&self) -> bool {
        self.server() == GROUP_SERVER
    }

    fn is_broadcast_list(&self) -> bool {
        self.server() == BROADCAST_SERVER && self.user() != STATUS_BROADCAST_USER
    }

    fn is_status_broadcast(&self) -> bool {
        self.server() == BROADCAST_SERVER && self.user() == STATUS_BROADCAST_USER
    }

    fn is_bot(&self) -> bool {
        (self.server() == DEFAULT_USER_SERVER
            && self.device() == 0
            && (self.user().starts_with("1313555") || self.user().starts_with("131655500")))
            || self.server() == BOT_SERVER
    }

    /// Returns true if this is a hosted/Cloud API device.
    /// Hosted devices have device ID 99 or use @hosted/@hosted.lid server.
    /// These devices should be excluded from group message fanout.
    fn is_hosted(&self) -> bool {
        self.device() == 99 || self.server() == HOSTED_SERVER || self.server() == HOSTED_LID_SERVER
    }

    fn is_empty(&self) -> bool {
        self.server().is_empty()
    }

    fn is_same_user_as(&self, other: &impl JidExt) -> bool {
        self.user() == other.user()
    }
}

#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct Jid {
    pub user: String,
    pub server: String,
    pub agent: u8,
    pub device: u16,
    pub integrator: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct JidRef<'a> {
    pub user: Cow<'a, str>,
    pub server: Cow<'a, str>,
    pub agent: u8,
    pub device: u16,
    pub integrator: u16,
}

impl JidExt for Jid {
    fn user(&self) -> &str {
        &self.user
    }
    fn server(&self) -> &str {
        &self.server
    }
    fn device(&self) -> u16 {
        self.device
    }
    fn integrator(&self) -> u16 {
        self.integrator
    }
}

impl Jid {
    pub fn new(user: &str, server: &str) -> Self {
        Self {
            user: user.to_string(),
            server: server.to_string(),
            ..Default::default()
        }
    }

    /// Create a phone number JID (s.whatsapp.net)
    pub fn pn(user: impl Into<String>) -> Self {
        Self {
            user: user.into(),
            server: DEFAULT_USER_SERVER.to_string(),
            ..Default::default()
        }
    }

    /// Create a LID JID (lid server)
    pub fn lid(user: impl Into<String>) -> Self {
        Self {
            user: user.into(),
            server: HIDDEN_USER_SERVER.to_string(),
            ..Default::default()
        }
    }

    /// Create a group JID (g.us)
    pub fn group(id: impl Into<String>) -> Self {
        Self {
            user: id.into(),
            server: GROUP_SERVER.to_string(),
            ..Default::default()
        }
    }

    /// Create a phone number JID with device ID
    pub fn pn_device(user: impl Into<String>, device: u16) -> Self {
        Self {
            user: user.into(),
            server: DEFAULT_USER_SERVER.to_string(),
            device,
            ..Default::default()
        }
    }

    /// Create a LID JID with device ID
    pub fn lid_device(user: impl Into<String>, device: u16) -> Self {
        Self {
            user: user.into(),
            server: HIDDEN_USER_SERVER.to_string(),
            device,
            ..Default::default()
        }
    }

    /// Returns true if this is a Phone Number based JID (s.whatsapp.net)
    #[inline]
    pub fn is_pn(&self) -> bool {
        self.server == DEFAULT_USER_SERVER
    }

    /// Returns true if this is a LID based JID
    #[inline]
    pub fn is_lid(&self) -> bool {
        self.server == HIDDEN_USER_SERVER
    }

    /// Returns the user part without the device ID suffix (e.g., "123:4" -> "123")
    #[inline]
    pub fn user_base(&self) -> &str {
        if let Some((base, _)) = self.user.split_once(':') {
            base
        } else {
            &self.user
        }
    }

    /// Helper to construct a specific device JID from this one
    pub fn with_device(&self, device_id: u16) -> Self {
        Self {
            user: self.user.clone(),
            server: self.server.clone(),
            agent: self.agent,
            device: device_id,
            integrator: self.integrator,
        }
    }

    pub fn actual_agent(&self) -> u8 {
        match self.server.as_str() {
            DEFAULT_USER_SERVER => 0,
            // For LID (HIDDEN_USER_SERVER), use the parsed agent value.
            // LID user identifiers can contain dots (e.g., "100000000000001.1"),
            // which are part of the identity, not agent separators.
            // Only non-device LID JIDs (without ':') may have an agent suffix.
            HIDDEN_USER_SERVER => self.agent,
            _ => self.agent,
        }
    }

    pub fn to_non_ad(&self) -> Self {
        Self {
            user: self.user.clone(),
            server: self.server.clone(),
            integrator: self.integrator,
            ..Default::default()
        }
    }

    /// Check if this JID matches the user or their LID.
    /// Useful for checking if a participant is "us" in group messages.
    #[inline]
    pub fn matches_user_or_lid(&self, user: &Jid, lid: Option<&Jid>) -> bool {
        self.is_same_user_as(user) || lid.is_some_and(|l| self.is_same_user_as(l))
    }

    pub fn to_ad_string(&self) -> String {
        if self.user.is_empty() {
            self.server.clone()
        } else {
            format!(
                "{}.{}:{}@{}",
                self.user, self.agent, self.device, self.server
            )
        }
    }
}

impl<'a> JidExt for JidRef<'a> {
    fn user(&self) -> &str {
        &self.user
    }
    fn server(&self) -> &str {
        &self.server
    }
    fn device(&self) -> u16 {
        self.device
    }
    fn integrator(&self) -> u16 {
        self.integrator
    }
}

impl<'a> JidRef<'a> {
    pub fn new(user: Cow<'a, str>, server: Cow<'a, str>) -> Self {
        Self {
            user,
            server,
            agent: 0,
            device: 0,
            integrator: 0,
        }
    }

    pub fn to_owned(&self) -> Jid {
        Jid {
            user: self.user.to_string(),
            server: self.server.to_string(),
            agent: self.agent,
            device: self.device,
            integrator: self.integrator,
        }
    }
}

impl FromStr for Jid {
    type Err = JidError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        // Try fast path first for well-formed JIDs
        if let Some(parts) = parse_jid_fast(s) {
            return Ok(Jid {
                user: parts.user.to_string(),
                server: parts.server.to_string(),
                agent: parts.agent,
                device: parts.device,
                integrator: parts.integrator,
            });
        }

        // Fallback to original parsing for edge cases and validation
        // Keep server as &str to avoid allocation until we need it
        let (user_part, server) = match s.split_once('@') {
            Some((u, s)) => (u, s),
            None => ("", s),
        };

        if user_part.is_empty() {
            let known_servers = [
                DEFAULT_USER_SERVER,
                GROUP_SERVER,
                LEGACY_USER_SERVER,
                BROADCAST_SERVER,
                HIDDEN_USER_SERVER,
                NEWSLETTER_SERVER,
                HOSTED_SERVER,
                MESSENGER_SERVER,
                INTEROP_SERVER,
                BOT_SERVER,
                STATUS_BROADCAST_USER,
            ];
            if !known_servers.contains(&server) {
                return Err(JidError::InvalidFormat(format!(
                    "Invalid JID format: unknown server '{}'",
                    server
                )));
            }
        }

        // Special handling for LID JIDs, as their user part can contain dots
        // that should not be interpreted as agent separators.
        if server == HIDDEN_USER_SERVER {
            let (user, device) = if let Some((u, d_str)) = user_part.rsplit_once(':') {
                (u, d_str.parse()?)
            } else {
                (user_part, 0)
            };
            return Ok(Jid {
                user: user.to_string(),
                server: server.to_string(),
                device,
                agent: 0,
                integrator: 0,
            });
        }

        // Fallback to existing logic for other JID types (s.whatsapp.net, etc.)
        let mut user = user_part;
        let mut device = 0;
        let mut agent = 0;

        if let Some((u, d_str)) = user_part.rsplit_once(':') {
            user = u;
            device = d_str.parse()?;
        }

        if server != DEFAULT_USER_SERVER
            && server != HIDDEN_USER_SERVER
            && let Some((u, last_part)) = user.rsplit_once('.')
            && let Ok(num_val) = last_part.parse::<u16>()
        {
            user = u;
            agent = num_val as u8;
        }

        if let Some((u, last_part)) = user_part.rsplit_once('.')
            && let Ok(num_val) = last_part.parse::<u16>()
        {
            if server == DEFAULT_USER_SERVER {
                user = u;
                device = num_val;
            } else {
                user = u;
                if num_val > u8::MAX as u16 {
                    return Err(JidError::InvalidFormat(format!(
                        "Agent component out of range: {num_val}"
                    )));
                }
                agent = num_val as u8;
            }
        }

        Ok(Jid {
            user: user.to_string(),
            server: server.to_string(),
            agent,
            device,
            integrator: 0,
        })
    }
}

impl fmt::Display for Jid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.user.is_empty() {
            // Server-only JID (e.g., "s.whatsapp.net") - no @ prefix
            write!(f, "{}", self.server)
        } else {
            write!(f, "{}", self.user)?;

            // The agent is encoded in the server type for AD JIDs.
            // We should NOT append it to the user string for standard servers.
            // Only non-standard servers might use an agent suffix.
            // The old JS logic appears to never append the agent for s.whatsapp.net or lid.
            if self.agent > 0 {
                // This is a guess based on the failure. The old JS logic is complex.
                // We will only append the agent if the server is NOT s.whatsapp.net or lid.
                // AND the server is not one that is derived *from* the agent (like 'hosted').
                let server_str = self.server(); // Use trait method
                if server_str != DEFAULT_USER_SERVER
                    && server_str != HIDDEN_USER_SERVER
                    && server_str != HOSTED_SERVER
                {
                    write!(f, ".{}", self.agent)?;
                }
            }

            if self.device > 0 {
                write!(f, ":{}", self.device)?;
            }

            write!(f, "@{}", self.server)
        }
    }
}

impl<'a> fmt::Display for JidRef<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.user.is_empty() {
            // Server-only JID (e.g., "s.whatsapp.net") - no @ prefix
            write!(f, "{}", self.server)
        } else {
            write!(f, "{}", self.user)?;

            // The agent is encoded in the server type for AD JIDs.
            // We should NOT append it to the user string for standard servers.
            // Only non-standard servers might use an agent suffix.
            // The old JS logic appears to never append the agent for s.whatsapp.net or lid.
            if self.agent > 0 {
                // This is a guess based on the failure. The old JS logic is complex.
                // We will only append the agent if the server is NOT s.whatsapp.net or lid.
                // AND the server is not one that is derived *from* the agent (like 'hosted').
                let server_str = self.server(); // Use trait method
                if server_str != DEFAULT_USER_SERVER
                    && server_str != HIDDEN_USER_SERVER
                    && server_str != HOSTED_SERVER
                {
                    write!(f, ".{}", self.agent)?;
                }
            }

            if self.device > 0 {
                write!(f, ":{}", self.device)?;
            }

            write!(f, "@{}", self.server)
        }
    }
}

impl From<Jid> for String {
    fn from(jid: Jid) -> Self {
        jid.to_string()
    }
}

impl<'a> From<JidRef<'a>> for String {
    fn from(jid: JidRef<'a>) -> Self {
        jid.to_string()
    }
}

impl TryFrom<String> for Jid {
    type Error = JidError;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        Jid::from_str(&value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    /// Helper function to test a full parsing and display round-trip.
    fn assert_jid_roundtrip(
        input: &str,
        expected_user: &str,
        expected_server: &str,
        expected_device: u16,
        expected_agent: u8,
    ) {
        assert_jid_parse_and_display(
            input,
            expected_user,
            expected_server,
            expected_device,
            expected_agent,
            input,
        );
    }

    /// Helper function to test parsing and display with a custom expected output.
    fn assert_jid_parse_and_display(
        input: &str,
        expected_user: &str,
        expected_server: &str,
        expected_device: u16,
        expected_agent: u8,
        expected_output: &str,
    ) {
        // 1. Test parsing from string (FromStr trait)
        let jid = Jid::from_str(input).unwrap_or_else(|_| panic!("Failed to parse JID: {}", input));

        assert_eq!(
            jid.user, expected_user,
            "User part did not match for {}",
            input
        );
        assert_eq!(
            jid.server, expected_server,
            "Server part did not match for {}",
            input
        );
        assert_eq!(
            jid.device, expected_device,
            "Device part did not match for {}",
            input
        );
        assert_eq!(
            jid.agent, expected_agent,
            "Agent part did not match for {}",
            input
        );

        // 2. Test formatting back to string (Display trait)
        let formatted = jid.to_string();
        assert_eq!(
            formatted, expected_output,
            "Formatted string did not match expected output for {}",
            input
        );
    }

    #[test]
    fn test_jid_parsing_and_display_roundtrip() {
        // Standard cases
        assert_jid_roundtrip(
            "1234567890@s.whatsapp.net",
            "1234567890",
            "s.whatsapp.net",
            0,
            0,
        );
        assert_jid_roundtrip(
            "1234567890:15@s.whatsapp.net",
            "1234567890",
            "s.whatsapp.net",
            15,
            0,
        );
        assert_jid_roundtrip("123-456@g.us", "123-456", "g.us", 0, 0);

        // Server-only JID: parsing "s.whatsapp.net" should display as "s.whatsapp.net" (no @ prefix)
        // This matches WhatsApp Web behavior where server-only JIDs don't have @ prefix
        assert_jid_roundtrip("s.whatsapp.net", "", "s.whatsapp.net", 0, 0);

        // LID JID cases (critical for the bug)
        assert_jid_roundtrip("12345.6789@lid", "12345.6789", "lid", 0, 0);
        assert_jid_roundtrip("12345.6789:25@lid", "12345.6789", "lid", 25, 0);
    }

    #[test]
    fn test_special_from_str_parsing() {
        // Test parsing of JIDs with an agent, which should be stored in the struct
        let jid = Jid::from_str("1234567890.2:15@hosted").expect("test hosted JID should be valid");
        assert_eq!(jid.user, "1234567890");
        assert_eq!(jid.server, "hosted");
        assert_eq!(jid.device, 15);
        assert_eq!(jid.agent, 2);
    }

    #[test]
    fn test_manual_jid_formatting_edge_cases() {
        // This test directly validates the fixes for the parity failures.
        // We manually construct the Jid struct as the binary decoder would,
        // then we assert that its string representation is correct.

        // Failure Case 1: An AD-JID for s.whatsapp.net decoded with an agent.
        // The Display trait MUST NOT show the agent number.
        let jid1 = Jid {
            user: "1234567890".to_string(),
            server: "s.whatsapp.net".to_string(),
            device: 15,
            agent: 2, // This agent would be decoded from binary but should be ignored in display
            integrator: 0,
        };
        // Expected: "1234567890:15@s.whatsapp.net" (agent is omitted)
        // Buggy: "1234567890.2:15@s.whatsapp.net"
        assert_eq!(jid1.to_string(), "1234567890:15@s.whatsapp.net");

        // Failure Case 2: A LID JID with a device, decoded with an agent.
        // The Display trait MUST NOT show the agent number.
        let jid2 = Jid {
            user: "12345.6789".to_string(),
            server: "lid".to_string(),
            device: 25,
            agent: 1, // This agent would be decoded from binary but should be ignored in display
            integrator: 0,
        };
        // Expected: "12345.6789:25@lid"
        // Buggy: "12345.6789.1:25@lid"
        assert_eq!(jid2.to_string(), "12345.6789:25@lid");

        // Failure Case 3: A JID that was decoded as "hosted" because of its agent.
        // The Display trait MUST NOT show the agent number.
        let jid3 = Jid {
            user: "1234567890".to_string(),
            server: "hosted".to_string(),
            device: 15,
            agent: 2,
            integrator: 0,
        };
        // Expected: "1234567890:15@hosted"
        // Buggy: "1234567890.2:15@hosted"
        assert_eq!(jid3.to_string(), "1234567890:15@hosted");

        // Verification Case: A generic JID where the agent SHOULD be displayed.
        let jid4 = Jid {
            user: "user".to_string(),
            server: "custom.net".to_string(),
            device: 10,
            agent: 5,
            integrator: 0,
        };
        // The agent should be displayed because the server is not a special AD-JID type
        assert_eq!(jid4.to_string(), "user.5:10@custom.net");
    }

    #[test]
    fn test_invalid_jids_should_fail_to_parse() {
        assert!(Jid::from_str("thisisnotajid").is_err());
        assert!(Jid::from_str("").is_err());
        // "@s.whatsapp.net" is now valid - it's the protocol format for server-only JIDs
        assert!(Jid::from_str("@s.whatsapp.net").is_ok());
        // But "@unknown.server" should still fail
        assert!(Jid::from_str("@unknown.server").is_err());
        // Jid::from_str("2") should not be possible due to type constraints,
        // but if it were, it should fail. The string must contain '@'.
        assert!(Jid::from_str("2").is_err());
    }

    /// Tests for HOSTED device detection (`is_hosted()` method).
    ///
    /// # Context: What are HOSTED devices?
    ///
    /// HOSTED devices (also known as Cloud API or Meta Business API devices) are
    /// WhatsApp Business accounts that use Meta's server-side infrastructure instead
    /// of traditional end-to-end encryption with Signal protocol.
    ///
    /// ## Key characteristics:
    /// - Device ID is always 99 (`:99`)
    /// - Server is `@hosted` (phone-based) or `@hosted.lid` (LID-based)
    /// - They do NOT use Signal protocol prekeys
    /// - They should be EXCLUDED from group message fanout
    /// - They CAN receive 1:1 messages (but prekey fetch will fail, causing graceful skip)
    ///
    /// ## Why exclude from groups?
    /// WhatsApp Web explicitly filters hosted devices from group SKDM (Sender Key
    /// Distribution Message) distribution. From WhatsApp Web JS (`getFanOutList`):
    /// ```javascript
    /// var isHosted = e.id === 99 || e.isHosted === true;
    /// var includeInFanout = !isHosted || isOneToOneChat;
    /// ```
    ///
    /// ## JID formats:
    /// - Phone-based: `5511999887766:99@hosted`
    /// - LID-based: `100000012345678:99@hosted.lid`
    /// - Regular device with ID 99: `5511999887766:99@s.whatsapp.net` (also hosted!)
    #[test]
    fn test_is_hosted_device_detection() {
        // === HOSTED DEVICES (should return true) ===

        // Case 1: Device ID 99 on regular server (Cloud API business account)
        // This is the most common case - a business using Meta's Cloud API
        let cloud_api_device: Jid = "5511999887766:99@s.whatsapp.net"
            .parse()
            .expect("test JID should be valid");
        assert!(
            cloud_api_device.is_hosted(),
            "Device ID 99 on s.whatsapp.net should be detected as hosted (Cloud API)"
        );

        // Case 2: Device ID 99 on LID server
        let cloud_api_lid: Jid = "100000012345678:99@lid"
            .parse()
            .expect("test JID should be valid");
        assert!(
            cloud_api_lid.is_hosted(),
            "Device ID 99 on lid server should be detected as hosted"
        );

        // Case 3: Explicit @hosted server (phone-based hosted JID)
        let hosted_server: Jid = "5511999887766:99@hosted"
            .parse()
            .expect("test JID should be valid");
        assert!(
            hosted_server.is_hosted(),
            "JID with @hosted server should be detected as hosted"
        );

        // Case 4: Explicit @hosted.lid server (LID-based hosted JID)
        let hosted_lid_server: Jid = "100000012345678:99@hosted.lid"
            .parse()
            .expect("test JID should be valid");
        assert!(
            hosted_lid_server.is_hosted(),
            "JID with @hosted.lid server should be detected as hosted"
        );

        // Case 5: @hosted server with different device ID (edge case)
        // Even with device ID != 99, if server is @hosted, it's a hosted device
        let hosted_server_other_device: Jid = "5511999887766:0@hosted"
            .parse()
            .expect("test JID should be valid");
        assert!(
            hosted_server_other_device.is_hosted(),
            "JID with @hosted server should be hosted regardless of device ID"
        );

        // === NON-HOSTED DEVICES (should return false) ===

        // Case 6: Regular phone device (primary phone, device 0)
        let regular_phone: Jid = "5511999887766:0@s.whatsapp.net"
            .parse()
            .expect("test JID should be valid");
        assert!(
            !regular_phone.is_hosted(),
            "Regular phone device (ID 0) should NOT be hosted"
        );

        // Case 7: Companion device (WhatsApp Web, device 33+)
        let companion_device: Jid = "5511999887766:33@s.whatsapp.net"
            .parse()
            .expect("test JID should be valid");
        assert!(
            !companion_device.is_hosted(),
            "Companion device (ID 33) should NOT be hosted"
        );

        // Case 8: Regular LID device
        let regular_lid: Jid = "100000012345678:0@lid"
            .parse()
            .expect("test JID should be valid");
        assert!(
            !regular_lid.is_hosted(),
            "Regular LID device should NOT be hosted"
        );

        // Case 9: LID companion device
        let lid_companion: Jid = "100000012345678:33@lid"
            .parse()
            .expect("test JID should be valid");
        assert!(
            !lid_companion.is_hosted(),
            "LID companion device (ID 33) should NOT be hosted"
        );

        // Case 10: Group JID (not a device at all)
        let group_jid: Jid = "120363012345678@g.us"
            .parse()
            .expect("test JID should be valid");
        assert!(
            !group_jid.is_hosted(),
            "Group JID should NOT be detected as hosted"
        );

        // Case 11: User JID without device
        let user_jid: Jid = "5511999887766@s.whatsapp.net"
            .parse()
            .expect("test JID should be valid");
        assert!(
            !user_jid.is_hosted(),
            "User JID without device should NOT be hosted"
        );

        // Case 12: Bot device
        let bot_jid: Jid = "13136555001:0@s.whatsapp.net"
            .parse()
            .expect("test JID should be valid");
        assert!(
            !bot_jid.is_hosted(),
            "Bot JID should NOT be detected as hosted (different mechanism)"
        );
    }

    /// Tests that document the filtering behavior for group messages.
    ///
    /// # Why this matters:
    /// When sending a group message, we distribute Sender Key Distribution Messages
    /// (SKDM) to all participant devices. However, HOSTED devices:
    /// 1. Don't use Signal protocol, so they can't process SKDM
    /// 2. WhatsApp Web explicitly excludes them from group fanout
    /// 3. Including them would cause unnecessary prekey fetch failures
    ///
    /// This test documents the expected behavior when filtering device lists.
    #[test]
    fn test_hosted_device_filtering_for_groups() {
        // Simulate a group with mixed device types
        let devices: Vec<Jid> = vec![
            // Regular devices that SHOULD receive SKDM
            "5511999887766:0@s.whatsapp.net"
                .parse()
                .expect("test JID should be valid"), // Phone
            "5511999887766:33@s.whatsapp.net"
                .parse()
                .expect("test JID should be valid"), // WhatsApp Web
            "5521988776655:0@s.whatsapp.net"
                .parse()
                .expect("test JID should be valid"), // Another user's phone
            "100000012345678:0@lid"
                .parse()
                .expect("test JID should be valid"), // LID device
            "100000012345678:33@lid"
                .parse()
                .expect("test JID should be valid"), // LID companion
            // HOSTED devices that should be EXCLUDED from group SKDM
            "5531977665544:99@s.whatsapp.net"
                .parse()
                .expect("test JID should be valid"), // Cloud API business
            "100000087654321:99@lid"
                .parse()
                .expect("test JID should be valid"), // Cloud API on LID
            "5541966554433:99@hosted"
                .parse()
                .expect("test JID should be valid"), // Explicit hosted
        ];

        // Filter out hosted devices (this is what prepare_group_stanza does)
        let filtered: Vec<&Jid> = devices.iter().filter(|jid| !jid.is_hosted()).collect();

        // Verify correct filtering
        assert_eq!(
            filtered.len(),
            5,
            "Should have 5 non-hosted devices after filtering"
        );

        // All filtered devices should NOT be hosted
        for jid in &filtered {
            assert!(
                !jid.is_hosted(),
                "Filtered list should not contain hosted devices: {}",
                jid
            );
        }

        // Count how many hosted devices were filtered out
        let hosted_count = devices.iter().filter(|jid| jid.is_hosted()).count();
        assert_eq!(hosted_count, 3, "Should have filtered out 3 hosted devices");
    }

    #[test]
    fn test_jid_pn_factory() {
        let jid = Jid::pn("1234567890");
        assert_eq!(jid.user, "1234567890");
        assert_eq!(jid.server, DEFAULT_USER_SERVER);
        assert_eq!(jid.device, 0);
        assert!(jid.is_pn());
    }

    #[test]
    fn test_jid_lid_factory() {
        let jid = Jid::lid("100000012345678");
        assert_eq!(jid.user, "100000012345678");
        assert_eq!(jid.server, HIDDEN_USER_SERVER);
        assert_eq!(jid.device, 0);
        assert!(jid.is_lid());
    }

    #[test]
    fn test_jid_group_factory() {
        let jid = Jid::group("123456789-1234567890");
        assert_eq!(jid.user, "123456789-1234567890");
        assert_eq!(jid.server, GROUP_SERVER);
        assert!(jid.is_group());
    }

    #[test]
    fn test_jid_pn_device_factory() {
        let jid = Jid::pn_device("1234567890", 5);
        assert_eq!(jid.user, "1234567890");
        assert_eq!(jid.server, DEFAULT_USER_SERVER);
        assert_eq!(jid.device, 5);
        assert!(jid.is_pn());
        assert!(jid.is_ad());
    }

    #[test]
    fn test_jid_lid_device_factory() {
        let jid = Jid::lid_device("100000012345678", 33);
        assert_eq!(jid.user, "100000012345678");
        assert_eq!(jid.server, HIDDEN_USER_SERVER);
        assert_eq!(jid.device, 33);
        assert!(jid.is_lid());
        assert!(jid.is_ad());
    }

    #[test]
    fn test_jid_factories_with_string_types() {
        // Test with &str
        let jid1 = Jid::pn("123");
        assert_eq!(jid1.user, "123");

        // Test with String
        let jid2 = Jid::lid(String::from("456"));
        assert_eq!(jid2.user, "456");

        // Test with owned String
        let user = "789".to_string();
        let jid3 = Jid::group(user);
        assert_eq!(jid3.user, "789");
    }
}
