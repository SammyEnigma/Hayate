//! Cryptographic primitives: X25519 ECDH key exchange, HKDF-SHA256 key
//! derivation, and AES-GCM / ChaCha20-Poly1305 AEAD.

use chacha20poly1305::aead::OsRng;
use chacha20poly1305::aead::rand_core::RngCore;
use ring::aead::{Aad, LessSafeKey, Nonce, UnboundKey};
use ring::hkdf;
use x25519_dalek::{EphemeralSecret, PublicKey};

use crate::EngineError;

pub mod features;

pub const CIPHER_CHACHA20: u8 = 0x00;
pub const CIPHER_AES256_GCM: u8 = 0x01;

pub const NONCE_LEN: usize = 12;
pub const TAG_LEN: usize = 16;

/// Prepared AEAD key for repeated frame operations.
///
/// Constructing a `ring::aead::LessSafeKey` expands and validates the selected
/// cipher key. Transfer workers process thousands of frames with the same
/// negotiated key, so they should build this once and reuse it for the worker's
/// lifetime.
pub struct AeadKey {
    inner: LessSafeKey,
}

impl AeadKey {
    /// Creates a reusable AEAD key from the negotiated transfer key.
    pub fn new(key: &[u8; 32], cipher_id: u8) -> Result<Self, EngineError> {
        let algo = get_algorithm(cipher_id)?;
        let unbound = UnboundKey::new(algo, key)
            .map_err(|_| EngineError::Crypto("failed to create UnboundKey".into()))?;
        Ok(Self {
            inner: LessSafeKey::new(unbound),
        })
    }
}

/// Resolves the AEAD algorithm for the given cipher ID.
pub fn get_algorithm(cipher_id: u8) -> Result<&'static ring::aead::Algorithm, EngineError> {
    match cipher_id {
        CIPHER_CHACHA20 => Ok(&ring::aead::CHACHA20_POLY1305),
        CIPHER_AES256_GCM => Ok(&ring::aead::AES_256_GCM),
        _ => Err(EngineError::Crypto("Unknown cipher suite requested".into())),
    }
}

/// Generates an ephemeral X25519 key pair.
#[must_use]
pub fn generate_keypair() -> (EphemeralSecret, [u8; 32]) {
    let secret = EphemeralSecret::random_from_rng(OsRng);
    let public = PublicKey::from(&secret);
    (secret, public.to_bytes())
}

/// Performs X25519 DH and derives a 32-byte symmetric key via HKDF-SHA256.
pub fn derive_key(
    secret: EphemeralSecret,
    peer_pub: &[u8; 32],
    passphrase: Option<&str>,
) -> Result<[u8; 32], EngineError> {
    let peer = PublicKey::from(*peer_pub);
    let shared = secret.diffie_hellman(&peer);
    let salt_bytes = match passphrase {
        Some(phrase) => phrase.as_bytes(),
        None => b"hayate-v2-default-salt",
    };
    let salt = hkdf::Salt::new(hkdf::HKDF_SHA256, salt_bytes);
    let prk = salt.extract(shared.as_bytes());
    let info: &[&[u8]] = &[b"hayate-v2-key"];
    let okm = prk
        .expand(info, hkdf::HKDF_SHA256)
        .map_err(|_| EngineError::Crypto("HKDF expand failed".into()))?;
    let mut key = [0u8; 32];
    okm.fill(&mut key)
        .map_err(|_| EngineError::Crypto("HKDF expand failed".into()))?;
    Ok(key)
}

/// Encrypts `plaintext` in-place inside `buf`, writing nonce + ciphertext + tag.
pub fn encrypt_frame<'buf>(
    key: &[u8; 32],
    cipher_id: u8,
    plaintext: &[u8],
    buf: &'buf mut Vec<u8>,
) -> Result<&'buf [u8], EngineError> {
    let aead = AeadKey::new(key, cipher_id)?;
    encrypt_frame_with_key(&aead, plaintext, buf)
}

/// Encrypts `plaintext` with an already prepared AEAD key.
pub fn encrypt_frame_with_key<'buf>(
    key: &AeadKey,
    plaintext: &[u8],
    buf: &'buf mut Vec<u8>,
) -> Result<&'buf [u8], EngineError> {
    let mut nonce_bytes = [0u8; NONCE_LEN];
    OsRng.fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::assume_unique_for_key(nonce_bytes);

    let start = buf.len();
    buf.extend_from_slice(&nonce_bytes);

    let plain_start = buf.len();
    buf.extend_from_slice(plaintext);
    let plain_end = buf.len();

    let tag = key
        .inner
        .seal_in_place_separate_tag(nonce, Aad::empty(), &mut buf[plain_start..plain_end])
        .map_err(|_| EngineError::Crypto("AEAD encrypt failed".into()))?;

    buf.extend_from_slice(tag.as_ref());

    Ok(&buf[start..])
}

/// Decrypts a frame produced by `encrypt_frame`.
pub fn decrypt_frame(key: &[u8; 32], cipher_id: u8, frame: &[u8]) -> Result<Vec<u8>, EngineError> {
    let mut out = Vec::with_capacity(frame.len());
    decrypt_frame_into(key, cipher_id, frame, &mut out)?;
    Ok(out)
}

/// Decrypts a frame produced by `encrypt_frame` into a reused buffer.
pub fn decrypt_frame_into(
    key: &[u8; 32],
    cipher_id: u8,
    frame: &[u8],
    out: &mut Vec<u8>,
) -> Result<(), EngineError> {
    let aead = AeadKey::new(key, cipher_id)?;
    decrypt_frame_into_with_key(&aead, frame, out)
}

/// Decrypts a frame with an already prepared AEAD key into a reused buffer.
pub fn decrypt_frame_into_with_key(
    key: &AeadKey,
    frame: &[u8],
    out: &mut Vec<u8>,
) -> Result<(), EngineError> {
    if frame.len() < NONCE_LEN + TAG_LEN {
        return Err(EngineError::Crypto("frame too short".into()));
    }
    let (nonce_bytes, rest) = frame.split_at(NONCE_LEN);
    let mut nonce_array = [0u8; NONCE_LEN];
    nonce_array.copy_from_slice(nonce_bytes);
    let nonce = Nonce::assume_unique_for_key(nonce_array);

    out.clear();
    out.extend_from_slice(rest);

    let plaintext_len = key
        .inner
        .open_in_place(nonce, Aad::empty(), out)
        .map_err(|_| EngineError::Crypto("AEAD decrypt failed".into()))?
        .len();
    out.truncate(plaintext_len);

    Ok(())
}

/// Encrypts a small metadata blob with a freshly-derived nonce.
pub fn encrypt_metadata(
    key: &[u8; 32],
    cipher_id: u8,
    plaintext: &[u8],
) -> Result<Vec<u8>, EngineError> {
    let mut out = Vec::with_capacity(NONCE_LEN + plaintext.len() + TAG_LEN);
    encrypt_frame(key, cipher_id, plaintext, &mut out)?;
    Ok(out)
}

/// Decrypts a metadata blob encrypted with `encrypt_metadata`.
pub fn decrypt_metadata(
    key: &[u8; 32],
    cipher_id: u8,
    data: &[u8],
) -> Result<Vec<u8>, EngineError> {
    decrypt_frame(key, cipher_id, data)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_phrase_key_derivation() {
        let (sec1, pub1) = generate_keypair();
        let (sec2, pub2) = generate_keypair();

        let phrase = "apple-bravo-charlie";

        // Correct phrase on both sides
        let key1 = derive_key(sec1, &pub2, Some(phrase)).unwrap();
        let key2 = derive_key(sec2, &pub1, Some(phrase)).unwrap();
        assert_eq!(key1, key2);

        // Different phrases
        let (sec3, pub3) = generate_keypair();
        let (sec4, pub4) = generate_keypair();
        let key3 = derive_key(sec3, &pub4, Some("apple-bravo-charlie")).unwrap();
        let key4 = derive_key(sec4, &pub3, Some("apple-bravo-delta")).unwrap();
        assert_ne!(key3, key4);

        // No phrase (default salt)
        let (sec5, pub5) = generate_keypair();
        let (sec6, pub6) = generate_keypair();
        let key5 = derive_key(sec5, &pub6, None).unwrap();
        let key6 = derive_key(sec6, &pub5, None).unwrap();
        assert_eq!(key5, key6);
        assert_ne!(key1, key5); // should differ from phrase-derived keys
    }

    #[test]
    fn test_metadata_encryption_and_decryption_mismatch() {
        let (sec_sender, pub_sender) = generate_keypair();
        let (sec_receiver, pub_receiver) = generate_keypair();

        let correct_phrase = "my-secret-phrase";
        let wrong_phrase = "wrong-secret-phrase";

        let sender_key = derive_key(sec_sender, &pub_receiver, Some(correct_phrase)).unwrap();
        let receiver_key_wrong = derive_key(sec_receiver, &pub_sender, Some(wrong_phrase)).unwrap();

        let plain_meta = b"metadata-payload";
        let encrypted = encrypt_metadata(&sender_key, CIPHER_CHACHA20, plain_meta).unwrap();

        // Decrypting with wrong key must fail
        let decrypt_res = decrypt_metadata(&receiver_key_wrong, CIPHER_CHACHA20, &encrypted);
        assert!(decrypt_res.is_err());
    }

    #[test]
    fn test_all_ciphers() {
        let key = [42u8; 32];
        let plain = b"hello world from cipher negotiation";

        for &cipher_id in &[CIPHER_CHACHA20, CIPHER_AES256_GCM] {
            let enc = encrypt_metadata(&key, cipher_id, plain).unwrap();
            let dec = decrypt_metadata(&key, cipher_id, &enc).unwrap();
            assert_eq!(plain, dec.as_slice());
        }
    }
}
