use crate::token::DICT_VERSION;

pub const NOISE_START_PATTERN: &str = "Noise_XX_25519_AESGCM_SHA256\x00\x00\x00\x00";

pub const WA_MAGIC_VALUE: u8 = 6;
pub const WA_CONN_HEADER: [u8; 4] = [b'W', b'A', WA_MAGIC_VALUE, DICT_VERSION];
