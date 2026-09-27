# 14: Store YouTube cookies and Spotify refresh token in Secret Service

**What to build:** Credentials are 0600 plaintext files (README security limitation). The manifest will need `--talk-name=org.freedesktop.secrets` again (removed in 25.09.1 while unused).

**Blocked by:** None (can start immediately)

**Status:** needs-triage

- [ ] Credentials stored via libsecret/oo7; plaintext files migrated and removed
- [ ] Sign-out deletes the secrets

## Comments

- 2026-09-27: `--talk-name=org.freedesktop.secrets` was removed from the manifest for 25.09.1 because nothing used it and it exposed the whole keyring (`.scratch/release-25.09.1/issues/10-unused-secret-service-permission.md`). Re-add it together with this work.
