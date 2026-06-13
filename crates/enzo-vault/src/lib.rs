//! Envelope-encrypted credential vault for Enzo.
//!
//! The vault stores database passwords (and other secrets) so that the user's master
//! password is the root of trust. The scheme is two-tier envelope encryption:
//!
//! ```text
//! master password --Argon2id(salt)--> Master Key (never stored)
//!                                          | unwraps
//!                                          v
//!                                   Vault Key (random, stored only wrapped)
//!                                          | XChaCha20-Poly1305 encrypts
//!                                          v
//!                              each secret (AAD-bound to its connection id)
//! ```
//!
//! Changing the master password re-wraps only the Vault Key, never the secrets. See
//! `design/security-credentials.md` for the full design.
//!
//! # Example
//!
//! ```
//! use enzo_vault::Vault;
//!
//! let mut vault = Vault::create("correct horse battery staple");
//! let key = vault.unlock("correct horse battery staple").unwrap();
//! vault.upsert_secret(&key, "prod-postgres", "s3cr3t");
//! let pw = vault.get_secret(&key, "prod-postgres").unwrap();
//! assert_eq!(&*pw, "s3cr3t");
//! ```

use base64::Engine as _;
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

const KEY_LEN: usize = 32;
const NONCE_LEN: usize = 24;
const SALT_LEN: usize = 16;
const PASSWORD_SLOT: &str = "password";
/// Associated data binding the wrapped vault key to its purpose.
const VK_AAD: &[u8] = b"enzo-vault-key/v1";

/// Errors returned by vault operations.
#[derive(Debug, thiserror::Error)]
pub enum VaultError {
    /// The configured key-derivation parameters are invalid (e.g. memory too low or
    /// salt too short).
    #[error("invalid KDF parameters")]
    InvalidKdfParams,
    /// Authenticated decryption failed: a wrong master password, a tampered
    /// ciphertext, or associated data that does not match.
    #[error("decryption failed (wrong password or tampered data)")]
    Decrypt,
    /// No secret is stored for the requested connection id.
    #[error("no secret stored for connection `{0}`")]
    SecretNotFound(String),
    /// The serialized vault is structurally corrupt.
    #[error("vault data is corrupt: {0}")]
    Corrupt(String),
}

/// An unwrapped vault key, held only in memory and zeroized on drop.
///
/// Obtain one with [`Vault::unlock`]. It is required to read or write secrets.
pub struct VaultKey(Zeroizing<[u8; KEY_LEN]>);

impl VaultKey {
    fn bytes(&self) -> &[u8; KEY_LEN] {
        &self.0
    }
}

// Redacted on purpose: the key bytes must never reach logs, panics, or `{:?}` output.
impl std::fmt::Debug for VaultKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("VaultKey").field(&"<redacted>").finish()
    }
}

/// Argon2id key-derivation parameters, persisted alongside the wrapped key.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct KdfParams {
    algo: String,
    salt_b64: String,
    mem_kib: u32,
    iters: u32,
    lanes: u32,
}

impl KdfParams {
    fn new_random() -> Self {
        Self {
            algo: "argon2id".to_owned(),
            salt_b64: b64(&random_bytes::<SALT_LEN>()),
            // OWASP-recommended Argon2id floor: 19 MiB, t=2, p=1.
            mem_kib: 19_456,
            iters: 2,
            lanes: 1,
        }
    }

    fn salt(&self) -> Result<Vec<u8>, VaultError> {
        unb64(&self.salt_b64)
    }
}

/// One wrapping of the vault key. Multiple slots (password, OS keystore, recovery)
/// can wrap the same key; any one unlocks it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct KeySlot {
    method: String,
    nonce_b64: String,
    wrapped_vk_b64: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    keystore_ref: Option<String>,
}

/// One encrypted secret, bound by AAD to its connection id and vault version.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct Secret {
    id: String,
    connection_id: String,
    aad: String,
    nonce_b64: String,
    ciphertext_b64: String,
}

/// An envelope-encrypted credential vault.
///
/// Serialize with [`Vault::to_json`] and persist to `vault.enc`; load with
/// [`Vault::from_json`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Vault {
    version: u32,
    kdf: KdfParams,
    key_slots: Vec<KeySlot>,
    secrets: Vec<Secret>,
}

impl Vault {
    /// Create a new vault protected by `master_password`.
    #[must_use]
    pub fn create(master_password: &str) -> Self {
        let kdf = KdfParams::new_random();
        let mk = derive_master_key(master_password.as_bytes(), &kdf)
            .expect("freshly generated KDF parameters are valid");
        let vk = Zeroizing::new(random_bytes::<KEY_LEN>());
        let (nonce, wrapped) = aead_encrypt(&mk, vk.as_slice(), VK_AAD);
        let slot = KeySlot {
            method: PASSWORD_SLOT.to_owned(),
            nonce_b64: b64(&nonce),
            wrapped_vk_b64: b64(&wrapped),
            keystore_ref: None,
        };
        Self {
            version: 1,
            kdf,
            key_slots: vec![slot],
            secrets: Vec::new(),
        }
    }

    /// Unlock the vault, returning the in-memory vault key.
    ///
    /// # Errors
    /// Returns [`VaultError::Decrypt`] for a wrong password, [`VaultError::Corrupt`]
    /// if the password slot is missing or malformed, or [`VaultError::InvalidKdfParams`]
    /// if the stored KDF parameters are unusable.
    pub fn unlock(&self, master_password: &str) -> Result<VaultKey, VaultError> {
        let mk = derive_master_key(master_password.as_bytes(), &self.kdf)?;
        let slot = self.password_slot()?;
        let nonce = decode_nonce(&slot.nonce_b64)?;
        let wrapped = unb64(&slot.wrapped_vk_b64)?;
        let vk = aead_decrypt(&mk, &nonce, &wrapped, VK_AAD)?;
        let arr: [u8; KEY_LEN] = vk
            .as_slice()
            .try_into()
            .map_err(|_| VaultError::Corrupt("vault key has wrong length".to_owned()))?;
        Ok(VaultKey(Zeroizing::new(arr)))
    }

    /// Insert or replace the secret for `connection_id`.
    pub fn upsert_secret(&mut self, key: &VaultKey, connection_id: &str, password: &str) {
        let aad = aad_for(connection_id, self.version);
        let (nonce, ct) = aead_encrypt(key.bytes(), password.as_bytes(), aad.as_bytes());
        let secret = Secret {
            id: new_secret_id(),
            connection_id: connection_id.to_owned(),
            aad,
            nonce_b64: b64(&nonce),
            ciphertext_b64: b64(&ct),
        };
        if let Some(existing) = self
            .secrets
            .iter_mut()
            .find(|s| s.connection_id == connection_id)
        {
            *existing = secret;
        } else {
            self.secrets.push(secret);
        }
    }

    /// Decrypt and return the secret for `connection_id`.
    ///
    /// # Errors
    /// Returns [`VaultError::SecretNotFound`] if absent, [`VaultError::Decrypt`] if the
    /// ciphertext or its associated data fails authentication, or
    /// [`VaultError::Corrupt`] if the stored fields are malformed.
    pub fn get_secret(
        &self,
        key: &VaultKey,
        connection_id: &str,
    ) -> Result<Zeroizing<String>, VaultError> {
        let secret = self
            .secrets
            .iter()
            .find(|s| s.connection_id == connection_id)
            .ok_or_else(|| VaultError::SecretNotFound(connection_id.to_owned()))?;
        let nonce = decode_nonce(&secret.nonce_b64)?;
        let ct = unb64(&secret.ciphertext_b64)?;
        let aad = aad_for(connection_id, self.version);
        let pt = aead_decrypt(key.bytes(), &nonce, &ct, aad.as_bytes())?;
        let text = String::from_utf8(pt.to_vec())
            .map_err(|_| VaultError::Corrupt("secret is not valid UTF-8".to_owned()))?;
        Ok(Zeroizing::new(text))
    }

    /// Remove the secret for `connection_id`. Returns `true` if one was removed.
    pub fn remove_secret(&mut self, connection_id: &str) -> bool {
        let before = self.secrets.len();
        self.secrets.retain(|s| s.connection_id != connection_id);
        self.secrets.len() != before
    }

    /// Re-wrap the vault key under a new master password. Secrets are untouched.
    ///
    /// # Errors
    /// Returns [`VaultError::Decrypt`] if `old_password` is wrong.
    pub fn change_master_password(
        &mut self,
        old_password: &str,
        new_password: &str,
    ) -> Result<(), VaultError> {
        let vk = self.unlock(old_password)?;
        let new_kdf = KdfParams::new_random();
        let new_mk = derive_master_key(new_password.as_bytes(), &new_kdf)
            .expect("freshly generated KDF parameters are valid");
        let (nonce, wrapped) = aead_encrypt(&new_mk, vk.bytes(), VK_AAD);
        self.kdf = new_kdf;
        // `unlock` above already proved a password slot exists, so this cannot fail.
        let slot = self
            .key_slots
            .iter_mut()
            .find(|s| s.method == PASSWORD_SLOT)
            .expect("unlock succeeded, so a password slot is present");
        slot.nonce_b64 = b64(&nonce);
        slot.wrapped_vk_b64 = b64(&wrapped);
        Ok(())
    }

    /// Serialize the vault to JSON for persistence.
    #[must_use]
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("vault is always serializable")
    }

    /// Parse a vault from JSON.
    ///
    /// # Errors
    /// Returns [`VaultError::Corrupt`] if the input is not a valid serialized vault.
    pub fn from_json(json: &str) -> Result<Self, VaultError> {
        serde_json::from_str(json).map_err(|e| VaultError::Corrupt(e.to_string()))
    }

    fn password_slot(&self) -> Result<&KeySlot, VaultError> {
        self.key_slots
            .iter()
            .find(|s| s.method == PASSWORD_SLOT)
            .ok_or_else(|| VaultError::Corrupt("missing password key slot".to_owned()))
    }
}

fn aad_for(connection_id: &str, version: u32) -> String {
    format!("{connection_id}|v{version}")
}

fn new_secret_id() -> String {
    format!("sec_{}", b64url(&random_bytes::<9>()))
}

fn derive_master_key(
    password: &[u8],
    params: &KdfParams,
) -> Result<Zeroizing<[u8; KEY_LEN]>, VaultError> {
    use argon2::{Algorithm, Argon2, Params, Version};

    let cfg = Params::new(params.mem_kib, params.iters, params.lanes, Some(KEY_LEN))
        .map_err(|_| VaultError::InvalidKdfParams)?;
    let argon = Argon2::new(Algorithm::Argon2id, Version::V0x13, cfg);
    let salt = params.salt()?;
    let mut key = Zeroizing::new([0u8; KEY_LEN]);
    argon
        .hash_password_into(password, &salt, key.as_mut_slice())
        .map_err(|_| VaultError::InvalidKdfParams)?;
    Ok(key)
}

fn aead_encrypt(key: &[u8; KEY_LEN], plaintext: &[u8], aad: &[u8]) -> ([u8; NONCE_LEN], Vec<u8>) {
    use chacha20poly1305::aead::{Aead, Payload};
    use chacha20poly1305::{Key, KeyInit, XChaCha20Poly1305, XNonce};

    let cipher = XChaCha20Poly1305::new(Key::from_slice(key));
    let nonce = random_bytes::<NONCE_LEN>();
    let ct = cipher
        .encrypt(
            XNonce::from_slice(&nonce),
            Payload {
                msg: plaintext,
                aad,
            },
        )
        .expect("XChaCha20-Poly1305 encryption is infallible for a valid key and nonce");
    (nonce, ct)
}

fn aead_decrypt(
    key: &[u8; KEY_LEN],
    nonce: &[u8; NONCE_LEN],
    ciphertext: &[u8],
    aad: &[u8],
) -> Result<Zeroizing<Vec<u8>>, VaultError> {
    use chacha20poly1305::aead::{Aead, Payload};
    use chacha20poly1305::{Key, KeyInit, XChaCha20Poly1305, XNonce};

    let cipher = XChaCha20Poly1305::new(Key::from_slice(key));
    let pt = cipher
        .decrypt(
            XNonce::from_slice(nonce),
            Payload {
                msg: ciphertext,
                aad,
            },
        )
        .map_err(|_| VaultError::Decrypt)?;
    Ok(Zeroizing::new(pt))
}

fn random_bytes<const N: usize>() -> [u8; N] {
    let mut buf = [0u8; N];
    getrandom::getrandom(&mut buf).expect("OS CSPRNG must be available");
    buf
}

fn decode_nonce(s: &str) -> Result<[u8; NONCE_LEN], VaultError> {
    let bytes = unb64(s)?;
    bytes
        .try_into()
        .map_err(|_| VaultError::Corrupt("nonce has wrong length".to_owned()))
}

fn b64(bytes: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

fn b64url(bytes: &[u8]) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

fn unb64(s: &str) -> Result<Vec<u8>, VaultError> {
    base64::engine::general_purpose::STANDARD
        .decode(s)
        .map_err(|e| VaultError::Corrupt(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    const PW: &str = "correct horse battery staple";

    fn unlocked() -> (Vault, VaultKey) {
        let vault = Vault::create(PW);
        let key = vault.unlock(PW).expect("unlock with correct password");
        (vault, key)
    }

    #[test]
    fn create_then_unlock_roundtrip() {
        let (mut vault, key) = unlocked();
        vault.upsert_secret(&key, "db", "hunter2");
        assert_eq!(&*vault.get_secret(&key, "db").unwrap(), "hunter2");
    }

    #[test]
    fn unlock_with_wrong_password_fails() {
        let vault = Vault::create(PW);
        let err = vault.unlock("wrong").unwrap_err();
        assert!(matches!(err, VaultError::Decrypt));
        assert_eq!(
            err.to_string(),
            "decryption failed (wrong password or tampered data)"
        );
    }

    #[test]
    fn upsert_replaces_existing_secret() {
        let (mut vault, key) = unlocked();
        vault.upsert_secret(&key, "db", "first");
        vault.upsert_secret(&key, "db", "second");
        assert_eq!(vault.secrets.len(), 1);
        assert_eq!(&*vault.get_secret(&key, "db").unwrap(), "second");
    }

    #[test]
    fn get_missing_secret_reports_connection() {
        let (vault, key) = unlocked();
        let err = vault.get_secret(&key, "nope").unwrap_err();
        assert!(matches!(err, VaultError::SecretNotFound(ref c) if c == "nope"));
        assert_eq!(err.to_string(), "no secret stored for connection `nope`");
    }

    #[test]
    fn remove_secret_reports_whether_present() {
        let (mut vault, key) = unlocked();
        vault.upsert_secret(&key, "db", "x");
        assert!(vault.remove_secret("db"));
        assert!(!vault.remove_secret("db"));
        assert!(matches!(
            vault.get_secret(&key, "db").unwrap_err(),
            VaultError::SecretNotFound(_)
        ));
    }

    #[test]
    fn aad_binding_blocks_swapped_ciphertexts() {
        let (mut vault, key) = unlocked();
        vault.upsert_secret(&key, "alpha", "alpha-pw");
        vault.upsert_secret(&key, "beta", "beta-pw");
        // Swap the encrypted payloads between the two connections.
        let (a_nonce, a_ct) = (
            vault.secrets[0].nonce_b64.clone(),
            vault.secrets[0].ciphertext_b64.clone(),
        );
        vault.secrets[0].nonce_b64 = vault.secrets[1].nonce_b64.clone();
        vault.secrets[0].ciphertext_b64 = vault.secrets[1].ciphertext_b64.clone();
        vault.secrets[1].nonce_b64 = a_nonce;
        vault.secrets[1].ciphertext_b64 = a_ct;
        // AAD no longer matches → authentication fails for both.
        assert!(matches!(
            vault.get_secret(&key, "alpha").unwrap_err(),
            VaultError::Decrypt
        ));
        assert!(matches!(
            vault.get_secret(&key, "beta").unwrap_err(),
            VaultError::Decrypt
        ));
    }

    #[test]
    fn tampered_ciphertext_fails_authentication() {
        let (mut vault, key) = unlocked();
        vault.upsert_secret(&key, "db", "secret");
        let mut raw = unb64(&vault.secrets[0].ciphertext_b64).unwrap();
        raw[0] ^= 0xFF;
        vault.secrets[0].ciphertext_b64 = b64(&raw);
        assert!(matches!(
            vault.get_secret(&key, "db").unwrap_err(),
            VaultError::Decrypt
        ));
    }

    #[test]
    fn change_password_keeps_secrets_and_rotates_auth() {
        let (mut vault, key) = unlocked();
        vault.upsert_secret(&key, "db", "keepme");
        vault.change_master_password(PW, "new-master-pw").unwrap();
        assert!(matches!(vault.unlock(PW).unwrap_err(), VaultError::Decrypt));
        let new_key = vault.unlock("new-master-pw").unwrap();
        assert_eq!(&*vault.get_secret(&new_key, "db").unwrap(), "keepme");
    }

    #[test]
    fn change_password_with_wrong_old_fails() {
        let mut vault = Vault::create(PW);
        assert!(matches!(
            vault
                .change_master_password("wrong", "whatever")
                .unwrap_err(),
            VaultError::Decrypt
        ));
    }

    #[test]
    fn json_roundtrip_preserves_behavior() {
        let (mut vault, key) = unlocked();
        vault.upsert_secret(&key, "db", "persisted");
        let json = vault.to_json();
        let loaded = Vault::from_json(&json).unwrap();
        assert_eq!(vault, loaded);
        let key2 = loaded.unlock(PW).unwrap();
        assert_eq!(&*loaded.get_secret(&key2, "db").unwrap(), "persisted");
    }

    #[test]
    fn from_json_rejects_garbage() {
        let err = Vault::from_json("not json").unwrap_err();
        assert!(matches!(err, VaultError::Corrupt(_)));
        assert!(err.to_string().starts_with("vault data is corrupt"));
    }

    #[test]
    fn unlock_with_corrupt_salt_fails() {
        let mut vault = Vault::create(PW);
        vault.kdf.salt_b64 = "!!!not-base64!!!".to_owned();
        assert!(matches!(
            vault.unlock(PW).unwrap_err(),
            VaultError::Corrupt(_)
        ));
    }

    #[test]
    fn unlock_with_invalid_kdf_memory_fails() {
        let mut vault = Vault::create(PW);
        vault.kdf.mem_kib = 1; // below Argon2's minimum.
        assert!(matches!(
            vault.unlock(PW).unwrap_err(),
            VaultError::InvalidKdfParams
        ));
    }

    #[test]
    fn unlock_with_too_short_salt_fails() {
        let mut vault = Vault::create(PW);
        vault.kdf.salt_b64 = b64(&[0u8; 4]); // valid base64, too short for Argon2.
        assert!(matches!(
            vault.unlock(PW).unwrap_err(),
            VaultError::InvalidKdfParams
        ));
    }

    #[test]
    fn unlock_without_password_slot_fails() {
        let mut vault = Vault::create(PW);
        vault.key_slots.clear();
        assert!(matches!(
            vault.unlock(PW).unwrap_err(),
            VaultError::Corrupt(_)
        ));
    }

    #[test]
    fn unlock_with_corrupt_wrapped_key_fails() {
        let mut vault = Vault::create(PW);
        vault.key_slots[0].wrapped_vk_b64 = "@@@".to_owned();
        assert!(matches!(
            vault.unlock(PW).unwrap_err(),
            VaultError::Corrupt(_)
        ));
    }

    #[test]
    fn unlock_with_wrong_length_nonce_fails() {
        let mut vault = Vault::create(PW);
        vault.key_slots[0].nonce_b64 = b64(&[0u8; 8]); // not 24 bytes.
        assert!(matches!(
            vault.unlock(PW).unwrap_err(),
            VaultError::Corrupt(_)
        ));
    }

    #[test]
    fn get_secret_with_corrupt_nonce_fails() {
        let (mut vault, key) = unlocked();
        vault.upsert_secret(&key, "db", "x");
        vault.secrets[0].nonce_b64 = b64(&[0u8; 3]);
        assert!(matches!(
            vault.get_secret(&key, "db").unwrap_err(),
            VaultError::Corrupt(_)
        ));
    }

    #[test]
    fn get_secret_with_corrupt_ciphertext_b64_fails() {
        let (mut vault, key) = unlocked();
        vault.upsert_secret(&key, "db", "x");
        vault.secrets[0].ciphertext_b64 = "*not*".to_owned();
        assert!(matches!(
            vault.get_secret(&key, "db").unwrap_err(),
            VaultError::Corrupt(_)
        ));
    }

    #[test]
    fn non_utf8_secret_is_reported_as_corrupt() {
        let (mut vault, key) = unlocked();
        // Encrypt non-UTF-8 bytes under the real key and AAD, then inject as a secret.
        let aad = aad_for("bin", vault.version);
        let (nonce, ct) = aead_encrypt(key.bytes(), &[0xFF, 0xFE, 0xFD], aad.as_bytes());
        vault.secrets.push(Secret {
            id: new_secret_id(),
            connection_id: "bin".to_owned(),
            aad,
            nonce_b64: b64(&nonce),
            ciphertext_b64: b64(&ct),
        });
        let err = vault.get_secret(&key, "bin").unwrap_err();
        assert!(matches!(err, VaultError::Corrupt(ref m) if m.contains("UTF-8")));
    }

    #[test]
    fn secret_ids_are_unique() {
        assert_ne!(new_secret_id(), new_secret_id());
    }

    #[test]
    fn unlock_with_wrong_length_vault_key_is_corrupt() {
        let mut vault = Vault::create(PW);
        // Re-wrap a 16-byte payload under the real master key: AEAD auth will succeed,
        // but the unwrapped "key" is the wrong length.
        let mk = derive_master_key(PW.as_bytes(), &vault.kdf).unwrap();
        let (nonce, wrapped) = aead_encrypt(&mk, &[7u8; 16], VK_AAD);
        vault.key_slots[0].nonce_b64 = b64(&nonce);
        vault.key_slots[0].wrapped_vk_b64 = b64(&wrapped);
        assert!(matches!(
            vault.unlock(PW).unwrap_err(),
            VaultError::Corrupt(_)
        ));
    }

    #[test]
    fn derives_and_redacted_debug_are_exercised() {
        let (mut vault, key) = unlocked();
        vault.upsert_secret(&key, "db", "x");
        // Clone + PartialEq + Debug on Vault, exercised transitively for the nested
        // KdfParams / KeySlot / Secret structs.
        let twin = vault.clone();
        assert_eq!(vault, twin);
        assert!(format!("{vault:?}").contains("Vault"));
        // The vault key must redact its bytes in Debug output.
        let shown = format!("{key:?}");
        assert!(shown.contains("redacted"));
    }

    #[test]
    fn change_password_leaves_a_single_password_slot() {
        let mut vault = Vault::create(PW);
        vault.change_master_password(PW, "next").unwrap();
        assert_eq!(
            vault
                .key_slots
                .iter()
                .filter(|s| s.method == PASSWORD_SLOT)
                .count(),
            1
        );
    }
}
