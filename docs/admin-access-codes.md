# Administrator access codes

Account detail responses and warehouse settings return the current access code
to the authorized administrator. The code stays visible and copyable after
reopening the profile and after server restarts. Code regeneration has no
cooldown; it changes the code only when the administrator explicitly requests it.

Authentication continues to use `mini_auth_accounts.credential_hash` (Argon2id).
Migration 0129 adds `mini_auth_code_vault` with an AES-256-GCM encrypted copy. A new
hash and its encrypted copy are committed in the same transaction. Ciphertexts
are authenticated against the principal and current hash, so a metadata update
cannot restore a previously replaced code. Admin detail reads never return hashes.

The encryption key is independent of PostgreSQL. On the configured Mac, the
Keychain launcher loads `access-code-encryption-key` from its configured Keychain
service into `MINI_ERP_ACCESS_CODE_KEY`. Elsewhere, supply that variable as a
base64-encoded 32-byte secret, or use the automatically created private
`data/access-code.key` file (`MINI_ERP_ACCESS_CODE_KEY_FILE` overrides the path).
Keep the same key across restarts and protect a recovery copy outside the database
backup. Restore the original key together with a database restore. Never commit
the key or generate a replacement during a normal deployment.

Previously migrated hash-only codes cannot be reconstructed from their hashes.
Known old codes can be restored for display without resetting them:

```sh
python3 tools/db/macos_keychain.py run -- target/release/mini_rs_auth_recover_codes \
  < /private/verified-code-candidates.json
```

The input is a JSON array of `{ "principal_ref": "...", "code": "..." }` objects.
Keep it private and pass it through stdin, never command arguments. Candidates
are checked against the current login hash; mismatches do not modify credentials.
The tool prints only the number recovered. No user is silently reset.

If an old code has no recoverable copy, the user's next successful ordinary login
preserves that same code for the administrator. This does not change the code or
require regeneration. A failed login never fills the vault. New and regenerated
codes are available immediately, including after restart.

Encryption uses the existing pinned [ring AEAD implementation](https://docs.rs/ring/0.17.14/ring/aead/struct.LessSafeKey.html).
