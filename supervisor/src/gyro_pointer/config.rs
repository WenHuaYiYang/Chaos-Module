//! GTP1/GTP2/GTP3 retain saved fields and gain defaults for newer preferences.
use super::logic::*;

pub const LENGTH: usize = 14;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Config {
    pub enabled: bool,
    pub sensitivity: u32,
    pub confirm_ms: u32,
    pub tolerance_px: u32,
    pub page_pause_s: u32,
}

impl Config {
    pub const fn defaults() -> Self {
        Self { enabled: false, sensitivity: SENSITIVITY_DEFAULT, confirm_ms: DWELL_MS,
               tolerance_px: TOLERANCE_DEFAULT_PX, page_pause_s: PAGE_PAUSE_DEFAULT_S }
    }
    pub fn valid(self) -> bool {
        (SENSITIVITY_MIN..=SENSITIVITY_MAX).contains(&self.sensitivity)
            && (CONFIRM_MIN_MS..=CONFIRM_MAX_MS).contains(&self.confirm_ms)
            && (TOLERANCE_MIN_PX..=TOLERANCE_MAX_PX).contains(&self.tolerance_px)
            && (PAGE_PAUSE_MIN_S..=PAGE_PAUSE_MAX_S).contains(&self.page_pause_s)
    }
    pub fn encode(self) -> [u8; LENGTH] {
        let mut bytes = [0u8; LENGTH];
        bytes[..4].copy_from_slice(b"GTP4"); bytes[4] = u8::from(self.enabled);
        bytes[5..7].copy_from_slice(&(self.sensitivity as u16).to_le_bytes());
        bytes[7..9].copy_from_slice(&(self.confirm_ms as u16).to_le_bytes());
        bytes[9..11].copy_from_slice(&(self.tolerance_px as u16).to_le_bytes());
        bytes[11..13].copy_from_slice(&(self.page_pause_s as u16).to_le_bytes());
        bytes[13] = bytes[..13].iter().fold(0xa5, |a, b| a ^ b);
        bytes
    }
    pub fn decode(bytes: &[u8]) -> Option<Self> {
        if bytes.len() == 5 && &bytes[..4] == b"GTP1" && bytes[4] <= 1 {
            return Some(Self { enabled: bytes[4] == 1, ..Self::defaults() });
        }
        let legacy = bytes.len() == 10 && &bytes[..4] == b"GTP2";
        let with_tolerance = bytes.len() == 12 && &bytes[..4] == b"GTP3";
        let current = bytes.len() == LENGTH && &bytes[..4] == b"GTP4";
        if !legacy && !with_tolerance && !current { return None; }
        let last = bytes.len()-1;
        if bytes[4] > 1 || bytes[..last].iter().fold(0xa5, |a, b| a ^ b) != bytes[last] { return None; }
        let config = Self {
            enabled: bytes[4] == 1,
            sensitivity: u16::from_le_bytes([bytes[5], bytes[6]]) as u32,
            confirm_ms: u16::from_le_bytes([bytes[7], bytes[8]]) as u32,
            tolerance_px: if with_tolerance || current { u16::from_le_bytes([bytes[9], bytes[10]]) as u32 }
                          else { TOLERANCE_DEFAULT_PX },
            page_pause_s: if current { u16::from_le_bytes([bytes[11], bytes[12]]) as u32 }
                          else { PAGE_PAUSE_DEFAULT_S },
        };
        if config.valid() { Some(config) } else { None }
    }
}

pub fn parse_number(text: &[u8]) -> Option<u32> {
    if text.is_empty() || text.len() > 4 { return None; }
    let mut value = 0;
    for c in text { if !c.is_ascii_digit() { return None; } value = value * 10 + (c-b'0') as u32; }
    Some(value)
}
