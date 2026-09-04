//! FinalShell connection password decryption.
//!
//! FinalShell stores SSH passwords as Base64(DES-ECB ciphertext) with an 8-byte header
//! prefix used to derive the DES key via Java `Random` + MD5. Hosts imported from
//! FinalShell into Agus without decryption will fail SSH auth unless decoded first.

use base64::Engine;
use cipher::{BlockDecrypt, KeyInit};
use des::Des;
use md5::{Digest, Md5};

const JAVA_RANDOM_MULTIPLIER: u64 = 0x5DEECE66D;
const JAVA_RANDOM_ADDEND: u64 = 0xB;
const JAVA_RANDOM_MASK: u64 = (1u64 << 48) - 1;
const KEY_SEED_BASE: i64 = 3_680_984_568_597_093_857;

fn signed_byte(value: u8) -> i8 {
    value as i8
}

struct JavaRandom {
    seed: u64,
}

impl JavaRandom {
    fn new(seed: i64) -> Self {
        Self {
            seed: (seed as u64 ^ JAVA_RANDOM_MULTIPLIER) & JAVA_RANDOM_MASK,
        }
    }

    fn next(&mut self, bits: u32) -> u32 {
        self.seed = (self
            .seed
            .wrapping_mul(JAVA_RANDOM_MULTIPLIER)
            .wrapping_add(JAVA_RANDOM_ADDEND))
            & JAVA_RANDOM_MASK;
        (self.seed >> (48 - bits)) as u32
    }

    fn next_int(&mut self, bound: i32) -> i32 {
        if bound <= 0 {
            return 0;
        }
        if (bound & (bound - 1)) == 0 {
            return ((bound as u64 * self.next(31) as u64) >> 31) as i32;
        }
        loop {
            let bits = self.next(31);
            let value = (bits % bound as u32) as i32;
            if bits
                .wrapping_sub(value as u32)
                .wrapping_add((bound - 1) as u32)
                < 0x8000_0000
            {
                return value;
            }
        }
    }

    fn next_long(&mut self) -> i64 {
        let high = (self.next(32) as i32) as i64;
        let low = (self.next(32) as i32) as i64;
        high.wrapping_shl(32).wrapping_add(low)
    }
}

fn random_key(head: &[u8; 8]) -> [u8; 8] {
    let mut seed_random = JavaRandom::new(i64::from(signed_byte(head[5])));
    let divisor = seed_random.next_int(127);
    let divisor = if divisor == 0 { 1 } else { divisor };
    let mut random = JavaRandom::new(KEY_SEED_BASE / i64::from(divisor));
    let skip_count = i32::from(signed_byte(head[0]));
    for _ in 0..skip_count {
        random.next_long();
    }
    let n = random.next_long();
    let mut second_random = JavaRandom::new(n);
    let values = [
        i64::from(signed_byte(head[4])),
        second_random.next_long(),
        i64::from(signed_byte(head[7])),
        i64::from(signed_byte(head[3])),
        second_random.next_long(),
        i64::from(signed_byte(head[1])),
        random.next_long(),
        i64::from(signed_byte(head[2])),
    ];
    let mut serialized = [0u8; 64];
    for (index, value) in values.iter().enumerate() {
        serialized[index * 8..(index + 1) * 8].copy_from_slice(&value.to_be_bytes());
    }
    let digest = Md5::digest(serialized);
    let mut key = [0u8; 8];
    key.copy_from_slice(&digest[..8]);
    key
}

fn des_ecb_pkcs5_decrypt(ciphertext: &[u8], key: &[u8; 8]) -> Option<Vec<u8>> {
    if ciphertext.is_empty() || !ciphertext.len().is_multiple_of(8) {
        return None;
    }
    let cipher = Des::new_from_slice(key).ok()?;
    let mut plaintext = Vec::with_capacity(ciphertext.len());
    for chunk in ciphertext.chunks(8) {
        let mut block = cipher::Block::<des::Des>::clone_from_slice(chunk);
        cipher.decrypt_block(&mut block);
        plaintext.extend_from_slice(&block);
    }
    let pad = *plaintext.last()?;
    if !(1..=8).contains(&pad) {
        return None;
    }
    if plaintext.len() < pad as usize {
        return None;
    }
    if !plaintext[plaintext.len() - pad as usize..]
        .iter()
        .all(|byte| *byte == pad)
    {
        return None;
    }
    plaintext.truncate(plaintext.len() - pad as usize);
    Some(plaintext)
}

/// Decrypt a FinalShell `password` field value to plaintext.
pub fn decode_finalshell_password(encrypted: &str) -> Option<String> {
    let trimmed = encrypted.trim();
    if trimmed.is_empty() {
        return None;
    }
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(trimmed)
        .ok()?;
    if bytes.len() <= 8 {
        return None;
    }
    let head: [u8; 8] = bytes[..8].try_into().ok()?;
    let ciphertext = &bytes[8..];
    let key = random_key(&head);
    let plaintext = des_ecb_pkcs5_decrypt(ciphertext, &key)?;
    String::from_utf8(plaintext)
        .ok()
        .filter(|value| is_plausible_password(value))
}

fn is_plausible_password(value: &str) -> bool {
    if value.is_empty() || value.len() > 256 {
        return false;
    }
    value.chars().all(|ch| !ch.is_control())
}

/// Returns true when the stored value is likely a FinalShell encrypted password blob.
pub fn looks_like_finalshell_password(value: &str) -> bool {
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed.starts_with("__secret__:") {
        return false;
    }
    let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(trimmed) else {
        return false;
    };
    bytes.len() > 8 && decode_finalshell_password(trimmed).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_known_finalshell_vectors() {
        assert_eq!(
            decode_finalshell_password("OGNqLj1Le11Br3AIelAiPaDJpfhBzmEN").as_deref(),
            Some("beac3d85988e")
        );
        assert_eq!(
            decode_finalshell_password("UU8hWV51DmVNgmX/pUd0LlaEo53VTa6s").as_deref(),
            Some("beac3d85988e")
        );
    }

    #[test]
    fn decodes_tencent_cloud_sample() {
        assert_eq!(
            decode_finalshell_password("OWZEYV5bBxDAsA8G4+MAoV5j7nFeQOvQFJxJ5rAcwHw=").as_deref(),
            Some("Yueli20190124Rye")
        );
    }

    #[test]
    fn rejects_plaintext_passwords() {
        assert!(!looks_like_finalshell_password("my-plain-password"));
    }
}
