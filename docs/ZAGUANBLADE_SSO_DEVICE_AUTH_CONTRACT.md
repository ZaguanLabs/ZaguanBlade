# ZaguanBlade SSO Device Auth Contract

**Status:** Mirrored Phase 0 contract from `ZaguanWebsite/docs/ZAGUANBLADE_SSO_DEVICE_AUTH_CONTRACT.md`.

ZaguanWebsite owns the device-auth endpoints. ZaguanBlade implements the native client. Keep this copy in sync with the Website contract when endpoint shapes change.

## Endpoints

`POST /api/blade/auth/start`

Request:

```json
{
  "device_name": "Stig's Framework Laptop",
  "platform": "linux",
  "app_version": "1.0.0"
}
```

Response:

```json
{
  "device_code": "opaque-256-bit-url-safe-secret",
  "user_code": "ABCD-EFGH",
  "verification_uri": "https://zaguanai.com/activate",
  "verification_uri_complete": "https://zaguanai.com/activate?code=ABCD-EFGH",
  "expires_in": 1800,
  "interval": 5
}
```

`POST /api/blade/auth/poll`

Request:

```json
{
  "device_code": "opaque-256-bit-url-safe-secret"
}
```

Responses:

```json
{ "status": "pending" }
```

```json
{ "status": "pending_subscription" }
```

```json
{
  "status": "approved",
  "api_key": "ps_live_xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx",
  "user_id": "canonical-prisma-user-id",
  "email": "user@example.com",
  "tier": "founders"
}
```

```json
{ "status": "denied" }
```

```json
{ "status": "expired" }
```

```json
{ "status": "consumed" }
```

## Client Rules

- Default Website base URL is `https://zaguanai.com`; support a dev/staging override.
- Open `verification_uri_complete` in the system browser.
- Poll no faster than `interval`; on `429`, wait `Retry-After`; on network/server errors, use exponential backoff capped at 30 seconds.
- Stop at `expires_in` and show restart affordance.
- `pending` means waiting for browser authorization.
- `pending_subscription` means the browser is in checkout or payment processing.
- `approved` returns the plaintext key once; store it immediately.
- `consumed` means the Website already delivered the key and Blade must restart the flow if the local store was not updated.

## Success Handling

On `approved`, Blade must:

1. Store `api_key` in the OS keychain, with owner-only file fallback.
2. Store `user_id`, `email`, and `tier` as non-secret account metadata.
3. Update AppState and the WebSocket credential path without requiring app restart.
4. Emit the existing remote settings changed event.

Manual API key entry remains supported and must not depend on this flow.
