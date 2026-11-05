# Runbook: rotating the encryption key

Cooper encrypts secrets at rest (two-factor secrets now; mailbox passwords and OAuth
tokens later) with the keys in `COOPER_ENCRYPTION_KEYS` (ADR-0013).

1. Generate a key: `openssl rand -base64 32`.
2. Put it **first** with a new ID, keep the current key after it, for example
   `COOPER_ENCRYPTION_KEYS=2:<new>,1:<old>`, in the configuration of **every**
   `cooper-server` and `cooper-worker` instance, and restart them. New values are now
   written with key 2; old values still decrypt with key 1.
3. Let the re-encryption jobs run (`identity-secrets-reencrypt`, every 10 minutes), or
   request a run from *Administration → Background Jobs*. Check that no value still uses
   the old key:

   ```sql
   SELECT count(*) FROM analysts
   WHERE totp_secret_encrypted NOT LIKE 'v2.%' OR totp_pending_encrypted NOT LIKE 'v2.%';
   ```

4. When the count is 0, remove the old key (`COOPER_ENCRYPTION_KEYS=2:<new>`) and restart.

Never delete a key that values still use: those secrets become unreadable (affected
Analysts would need an Administrator to reset their two-factor sign-in).
