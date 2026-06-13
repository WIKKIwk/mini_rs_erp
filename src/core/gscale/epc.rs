use std::fs::File;
use std::io::Read;
use std::process;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use super::ports::EpcSource;

#[derive(Debug)]
pub struct GscaleEpcGenerator {
    state: Mutex<EpcState>,
}

#[derive(Debug)]
struct EpcState {
    last_ns: i64,
    seq: u32,
    salt: u32,
}

impl GscaleEpcGenerator {
    pub fn new() -> Self {
        Self::with_salt(new_epc_salt())
    }

    pub fn with_salt(salt: u32) -> Self {
        Self {
            state: Mutex::new(EpcState {
                last_ns: 0,
                seq: 0,
                salt: salt | 1,
            }),
        }
    }

    pub fn next_at_unix_ns(&self, ns: i64) -> String {
        let mut state = self.state.lock().expect("epc generator mutex");
        if ns != state.last_ns {
            state.last_ns = ns;
            state.seq = 0;
        } else {
            state.seq = state.seq.wrapping_add(1);
        }
        format_epc_24(ns, state.seq, state.salt)
    }
}

impl Default for GscaleEpcGenerator {
    fn default() -> Self {
        Self::new()
    }
}

impl EpcSource for GscaleEpcGenerator {
    fn next_epc(&self) -> String {
        self.next_at_unix_ns(now_unix_ns())
    }
}

pub fn format_epc_24(ns: i64, seq: u32, salt: u32) -> String {
    let ns_bits = ns as u64;
    let atom = ((ns_bits / 1_000) & 0xFFFF_FFFF) as u32;
    let mut tail = atom ^ (ns as u32).rotate_left(13) ^ seq.rotate_left(7) ^ salt;
    tail |= 1;
    format!("30{:014X}{:08X}", ns_bits & 0x00FF_FFFF_FFFF_FFFF, tail)
}

fn now_unix_ns() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
        .min(i64::MAX as u128) as i64
}

fn new_epc_salt() -> u32 {
    read_os_random_u32().unwrap_or_else(|| now_unix_ns() as u32 ^ (process::id() << 16)) | 1
}

fn read_os_random_u32() -> Option<u32> {
    let mut bytes = [0_u8; 4];
    let mut file = File::open("/dev/urandom").ok()?;
    file.read_exact(&mut bytes).ok()?;
    Some(u32::from_be_bytes(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_epc_like_gscale_and_rps() {
        assert_eq!(
            format_epc_24(1_691_139_600_123_456_789, 0, 0x1357_9BDF),
            "307822819AF46D1581D4AEC1"
        );
        assert_eq!(
            format_epc_24(1_691_139_600_123_456_789, 1, 0x1357_9BDF),
            "307822819AF46D1581D4AE41"
        );
    }

    #[test]
    fn increments_sequence_for_same_timestamp() {
        let generator = GscaleEpcGenerator::with_salt(0x1357_9BDE);
        let ns = 1_691_139_600_123_456_789;

        assert_eq!(generator.next_at_unix_ns(ns), "307822819AF46D1581D4AEC1");
        assert_eq!(generator.next_at_unix_ns(ns), "307822819AF46D1581D4AE41");
    }
}
