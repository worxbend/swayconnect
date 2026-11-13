//! Proofs, the device key and `confirm_code` (`docs/PROTOCOL.md` 5.3).
//!
//! `S` and `K` are secrets. Nothing here logs or formats them.

use ring::hmac;

use crate::message::DeviceId;

const PAIR_LABEL: &str = "swayconnect/1 pair";
const KEY_LABEL: &str = "swayconnect/1 key";
const CONFIRM_LABEL: &str = "swayconnect/1 confirm";
const CLIENT_LABEL: &str = "swayconnect/1 client";
const SERVER_LABEL: &str = "swayconnect/1 server";

const CONFIRM_CODE_MODULUS: u32 = 1_000_000;

/// `ctx`: the channel binding and both nonces of one connection.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Context {
    pub fp: [u8; 32],
    pub server_nonce: [u8; 32],
    pub client_nonce: [u8; 32],
}

/// The system random source failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("the system random source failed")]
pub struct RandomError;

/// `N` bytes from the system's secure random source, for nonces, `S` and device ids.
pub fn random_bytes<const N: usize>() -> Result<[u8; N], RandomError> {
    use ring::rand::SecureRandom as _;

    let mut bytes = [0u8; N];
    ring::rand::SystemRandom::new()
        .fill(&mut bytes)
        .map_err(|_| RandomError)?;
    Ok(bytes)
}

pub fn pair_proof(secret: &[u8; 32], context: &Context) -> [u8; 32] {
    mac(secret, &message(PAIR_LABEL, context, None))
}

/// `K`. Computed once, at pairing.
pub fn device_key(secret: &[u8; 32], context: &Context) -> [u8; 32] {
    mac(secret, &message(KEY_LABEL, context, None))
}

pub fn confirm_mac(device_key: &[u8; 32], context: &Context) -> [u8; 32] {
    mac(device_key, &message(CONFIRM_LABEL, context, None))
}

/// Six decimal digits. Not a secret.
pub fn confirm_code(device_key: &[u8; 32], context: &Context) -> String {
    let [a, b, c, d, ..] = confirm_mac(device_key, context);
    let number = u32::from_be_bytes([a, b, c, d]) % CONFIRM_CODE_MODULUS;
    format!("{number:06}")
}

pub fn client_proof(device_key: &[u8; 32], context: &Context, device_id: &DeviceId) -> [u8; 32] {
    mac(device_key, &message(CLIENT_LABEL, context, Some(device_id)))
}

pub fn server_proof(device_key: &[u8; 32], context: &Context, device_id: &DeviceId) -> [u8; 32] {
    mac(device_key, &message(SERVER_LABEL, context, Some(device_id)))
}

/// Constant-time check of `pair.proof`.
#[must_use]
pub fn verify_pair_proof(secret: &[u8; 32], context: &Context, proof: &[u8; 32]) -> bool {
    verify(secret, &message(PAIR_LABEL, context, None), proof)
}

/// Constant-time check of `auth.proof`.
#[must_use]
pub fn verify_client_proof(
    device_key: &[u8; 32],
    context: &Context,
    device_id: &DeviceId,
    proof: &[u8; 32],
) -> bool {
    let message = message(CLIENT_LABEL, context, Some(device_id));
    verify(device_key, &message, proof)
}

/// Constant-time check of `welcome.server_proof`.
#[must_use]
pub fn verify_server_proof(
    device_key: &[u8; 32],
    context: &Context,
    device_id: &DeviceId,
    proof: &[u8; 32],
) -> bool {
    let message = message(SERVER_LABEL, context, Some(device_id));
    verify(device_key, &message, proof)
}

/// `label || 0x00 || ctx`, then the ASCII bytes of the device id where the proof has one.
fn message(label: &str, context: &Context, device_id: Option<&DeviceId>) -> Vec<u8> {
    let mut message = Vec::with_capacity(label.len() + 1 + 96 + 22);
    message.extend_from_slice(label.as_bytes());
    message.push(0);
    message.extend_from_slice(&context.fp);
    message.extend_from_slice(&context.server_nonce);
    message.extend_from_slice(&context.client_nonce);
    if let Some(device_id) = device_id {
        message.extend_from_slice(device_id.as_str().as_bytes());
    }
    message
}

fn mac(key: &[u8; 32], message: &[u8]) -> [u8; 32] {
    let tag = hmac::sign(&hmac::Key::new(hmac::HMAC_SHA256, key), message);
    let mut output = [0u8; 32];
    output.copy_from_slice(tag.as_ref());
    output
}

fn verify(key: &[u8; 32], message: &[u8], proof: &[u8; 32]) -> bool {
    hmac::verify(&hmac::Key::new(hmac::HMAC_SHA256, key), message, proof).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn random_bytes_differ_between_calls() {
        let first: [u8; 32] = random_bytes().expect("system random works");
        let second: [u8; 32] = random_bytes().expect("system random works");
        assert_ne!(first, second);
    }
}
