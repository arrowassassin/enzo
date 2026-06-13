# Enzo — Credential Encryption Framework

> How Enzo stores database passwords (and other secrets) so that **the user's Enzo
> master password is the root private key**, with the OS keystore as a convenience
> unlock path — never a single point of failure.

Version: 0.1 (draft) · Last updated: 2026-06-13

---

## 1. Goals & threat model

### Protect against
- A stolen disk, backup, or accidentally-synced/committed config file.
- Another local user reading Enzo's files.
- Malware reading files at rest while Enzo is **locked**.
- Secrets leaking into logs, scrollback, or AI/agent context.

### Explicitly out of scope (true for every tool of this class)
- A compromised OS kernel / active keylogger while Enzo is **unlocked**.
- A user who installs a malicious plugin and grants it secret access.

### Hard rules
- DB passwords are **never** stored in plaintext, nor in any form Enzo alone (the
  app/the developers) can decrypt. Decryption requires a user secret.
- Secrets exist in plaintext **only in locked memory**, are **zeroized** after use,
  and are **never** written to disk, logs, scrollback, or sent to an agent.

---

## 2. Cryptographic primitives (all memory-safe, audited, $0)

| Purpose | Algorithm | Crate |
|---|---|---|
| Password → key (KDF) | **Argon2id** (memory-hard) | `argon2` (RustCrypto) |
| Authenticated encryption | **XChaCha20-Poly1305** (192-bit nonce → random nonces safe) | audited impl preferred: **`dryoc`** (libsodium) or `ring`; `chacha20poly1305` (RustCrypto) acceptable |
| Key wrapping | XChaCha20-Poly1305 (or AES-256-KW) | same |
| Randomness | OS CSPRNG | `getrandom` |
| In-memory secret handling | wipe-on-drop + typed secrets | `zeroize`, `secrecy` |
| Memory locking (no swap) | `mlock` / `VirtualLock` | `region` / `memsec` |

**Deliberately NOT OpenSSL** — its CVE history is the exact thing to avoid. Pure-
Rust RustCrypto primitives + `ring` where a vetted assembly impl is wanted.

Argon2id parameters (tune to ~0.5s on target hardware): memory ≥ 256 MiB,
iterations t = 3, parallelism p = 4. Parameters are stored in the vault header so
they can be raised over time and re-derived on unlock.

---

## 3. Key hierarchy (envelope encryption)

Two tiers, so changing the master password never re-encrypts every secret, and
multiple unlock paths can coexist.

```
   master password ──Argon2id(salt,params)──►  MASTER KEY (MK)   [never stored]
                                                     │ unwraps
   OS device key ──(keystore, OS-auth gated)──►      │
   recovery code ──Argon2id──────────────────►       ▼
                                            VAULT KEY (VK)  [random 256-bit,
                                                            stored only wrapped]
                                                     │ encrypts
                                                     ▼
        each DB password = XChaCha20-Poly1305(VK, nonce, password, aad=conn_id)
```

- **Master Key (MK)** — derived on demand from the master password. Never stored.
- **Vault Key (VK)** — a random 256-bit data key generated once at vault creation.
  It actually encrypts the secrets. VK is stored **only in wrapped form**, in one or
  more "key slots."
- **Key slots** — each slot is an independent wrapping of the *same* VK:
  - `password` slot: VK wrapped by MK (the root; always present; portable).
  - `os-keystore` slot: VK wrapped by a high-entropy **device key** held in the OS
    keystore, gated by OS auth (Touch ID / Windows Hello / login keyring).
  - `recovery` slot (optional): VK wrapped by a key derived from a printed recovery
    code.

Any slot can unlock. Adding biometrics or changing the master password only
re-wraps VK in one slot — the secrets are untouched.

### Why this matches "Enzo's password acts as the private key"
The master password is the **root of trust**: the `password` slot always exists and
can unlock on any machine. The OS keystore slot is a *convenience* — it lets you
unlock with Touch ID without typing the password on your trusted device — but it is
never the sole holder of the secrets. Lose the keystore (reinstall OS) → master
password still works. Lose the master password → only the recovery code can help;
**by design, Enzo cannot recover it** (zero-knowledge).

---

## 4. OS-native secure storage (the keystore slot)

The keystore stores **only the device key (or the wrapped VK)** — never the DB
passwords themselves.

| OS | Storage | Auth gate | Hardware backing | Rust |
|---|---|---|---|---|
| **macOS** | **Keychain Services** (Security.framework) | `SecAccessControl` → Touch ID / passcode (`LAContext`) | **Secure Enclave** (`kSecAttrTokenIDSecureEnclave`) — device key never leaves the enclave | `security-framework` |
| **Windows** | **DPAPI** (`CryptProtectData`, user-scoped) baseline; **Credential Manager** (`CredWrite`) for storage | **Windows Hello** (`KeyCredentialManager`) | **TPM** via CNG `NCrypt` + Microsoft Platform Crypto Provider | `windows` (windows-rs) |
| **Linux** | **Secret Service** (libsecret / GNOME Keyring / KWallet) over D-Bus | login keyring unlock | optional **TPM 2.0** via tpm2-tss | `secret-service` |

**Linux fallback** (no Secret Service present, e.g. headless): fall back to the
kernel keyring (`keyutils`) for the session, or to a **password-only vault** (no
keystore slot) with an explicit warning to the user. Never silently downgrade.

The Windows answer to "what replaces Keychain": **DPAPI for at-rest user-bound
encryption + Windows Hello/TPM for the biometric-gated device key + Credential
Manager for storage.** That trio is the functional equivalent of macOS Keychain +
Secure Enclave.

---

## 5. Vault file format

Location: `~/.config/enzo/vault.enc` (and platform equivalents). Permissions
`0600`. **Never synced, never committed** (shipped `.gitignore` + a sync-folder
warning). The OS keystore holds only the device key / a reference.

```jsonc
{
  "version": 1,
  "kdf": { "algo": "argon2id", "salt": "<b64>",
           "mem_kib": 262144, "iters": 3, "lanes": 4 },
  "key_slots": [
    { "method": "password",     "nonce": "<b64>", "wrapped_vk": "<b64>" },
    { "method": "os-keystore",  "nonce": "<b64>", "wrapped_vk": "<b64>",
      "keystore_ref": "enzo.device-key" },
    { "method": "recovery",     "nonce": "<b64>", "wrapped_vk": "<b64>" }
  ],
  "secrets": [
    { "id": "sec_01H...", "connection_id": "conn_prod_pg",
      "nonce": "<b64>", "aad": "conn_prod_pg|v1", "ciphertext": "<b64>" }
  ]
}
```

The **AAD binds each ciphertext to its connection id and vault version**, so a
ciphertext can't be swapped between entries or replayed across vaults.

---

## 6. Workflows

### 6.1 First-time setup
1. User sets an Enzo master password (strength-metered; never sent anywhere).
2. Generate random salt → derive MK (Argon2id).
3. Generate random VK; wrap with MK → `password` slot.
4. Offer biometric unlock → generate device key in Secure Enclave/TPM, wrap VK →
   `os-keystore` slot, store device key reference in the OS keystore.
5. Offer to generate a one-time **recovery code** (shown once) → `recovery` slot.

### 6.2 Unlock (on launch / after auto-lock)
```
 launch ─► vault locked
   │
   ├─ try os-keystore slot ─► OS prompts Touch ID / Hello ─► get device key
   │                          ─► unwrap VK ─► UNLOCKED
   │
   └─ else prompt master password ─► Argon2id ─► MK ─► unwrap VK ─► UNLOCKED
```
VK is held in a `secrecy::Secret`, in `mlock`ed memory, for the session. Wrong
password: Argon2id makes brute force expensive; add per-attempt backoff.

### 6.3 Connecting to a database
1. Look up the encrypted secret for `connection_id`.
2. Decrypt with VK in locked memory (verify AAD = `connection_id|version`).
3. Hand the plaintext password to the driver (ADBC/sqlx) over an in-process call.
4. **Zeroize** the plaintext immediately after the driver consumes it.
   Never logged, never in scrollback, never in an ATP block.

### 6.4 Saving a new credential
Encrypt with VK + fresh random nonce + AAD(`connection_id|version`); append to
`secrets`. VK never leaves memory.

### 6.5 Changing the master password
Derive MK' from the new password → re-wrap VK → replace the `password` slot. VK and
all `secrets` untouched. (Same pattern to add/remove a biometric or revoke a
recovery code: add/remove a slot.)

### 6.6 Auto-lock
VK is zeroized and the vault relocks on: inactivity timeout (configurable), system
sleep/lock, or manual lock (`> lock vault`). Connecting again requires re-unlock.

---

## 7. Secret hygiene across the rest of Enzo

This is where most "secure" tools leak. Enforced rules:
- **Never in scrollback.** When the agent or a command would echo a password (e.g.
  a connection string), the daemon redacts it before it reaches the grid.
- **Never in agent context.** The reference/block layer runs a redaction pass:
  connection strings, `password=`, tokens, and the vault's own contents are scrubbed
  before any `ref`/`block` is sent over ATP. The agent sees `password=••••••`.
- **Never in plugins by default.** WASM plugins get vault access only via an
  explicit, separately-granted capability — and even then, only an opaque "connect
  using connection X" call, not the raw password.
- **Never in the browser process.** CEF is sandboxed with no IPC path to the vault.
- **Memory discipline.** All secret types are `Zeroize`/`ZeroizeOnDrop`, wrapped in
  `secrecy::Secret`, and held in `mlock`ed pages to keep them out of swap and crash
  dumps.

---

## 8. Supply-chain & build security
- `cargo-deny` (license + advisory ban policy) and `cargo-audit` (RustSec) on every
  CI run; `cargo-vet` for first-party dependency review.
- Minimal dependency tree; pinned + vendored; no unreviewed transitive updates.
- Published SBOM and reproducible builds so users can verify the binary == source.
- Signed releases.

---

## 9. What we explicitly do NOT do
- Do not implement custom crypto primitives — only vetted library algorithms.
- Do not store, transmit, or escrow the master password anywhere.
- Do not provide a developer/vendor backdoor to recover secrets (zero-knowledge).
- Do not auto-sync the vault to any cloud.
