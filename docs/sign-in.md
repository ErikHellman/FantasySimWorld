# Signing in with Google and Apple

The server has three modes:

- `AUTH_MODE=off`: no sign-in; everyone is an admin. For local development only: the server refuses to start unless it listens on a loopback address (the default becomes `127.0.0.1:8080`) and `PUBLIC_ORIGIN` is `http://localhost` or a loopback IP, so a deployment can never run open.
- `AUTH_MODE=token` (the default): one shared secret in `AUTH_TOKEN` (16+ characters). Whoever opens `/auth/token` and types it gets a cookie and is an admin. Good for local work and a first deployment.
- `AUTH_MODE=oidc`: Google and/or Apple sign-in, and only e-mail addresses on the allowlist get in.

This page is about the last. Set `PUBLIC_ORIGIN` to the address people type (for example `https://village.example.com`): the redirect URIs below are built from it and must match exactly.

## Google

1. In the [Google Cloud console](https://console.cloud.google.com/apis/credentials), create a project (or pick one), then **Create credentials → OAuth client ID → Web application**.
2. Under **Authorized redirect URIs** add `<PUBLIC_ORIGIN>/auth/google/callback`. Add `http://localhost:8080/auth/google/callback` too if you want to try it locally (Google allows plain `http` for `localhost` only).
3. On the **OAuth consent screen**, the scopes are `openid`, `email` and `profile`, which need no review. While the app is in "Testing", only the test users you list there can sign in at all; publish it to let anyone reach the allowlist check.
4. Give the server the client's ID and secret:

```bash
AUTH_MODE=oidc
AUTH_PROVIDERS=google
GOOGLE_CLIENT_ID=<…>.apps.googleusercontent.com
GOOGLE_CLIENT_SECRET=<…>
```

## Apple

Apple only redirects to a real `https` domain, so Apple sign-in can be tried only on the deployed server.

1. In [Certificates, Identifiers & Profiles](https://developer.apple.com/account/resources/identifiers/list), make sure an **App ID** has *Sign in with Apple* enabled.
2. Create a **Services ID** (this is the client ID, e.g. `com.example.village.web`), enable *Sign in with Apple* on it, and under **Configure** pick that App ID, add your domain (without `https://`) and the return URL `<PUBLIC_ORIGIN>/auth/apple/callback`.
3. Create a **Key** with *Sign in with Apple* enabled for that App ID and download the `.p8` file (it can be downloaded only once). Note its Key ID and your Team ID (top right of the developer site).
4. Give the server:

```bash
AUTH_MODE=oidc
AUTH_PROVIDERS=apple            # or google,apple
APPLE_TEAM_ID=<10 characters>
APPLE_KEY_ID=<10 characters>
APPLE_SERVICES_ID=com.example.village.web
APPLE_PRIVATE_KEY="$(cat AuthKey_XXXXXXXXXX.p8)"   # "\n" may stand for line breaks
```

People who choose **Hide My Email** arrive with an `…@privaterelay.appleid.com` address. That is the address to allow; it shows up under refused sign-ins after their first try.

## The allowlist

- `ADMIN_EMAILS=a@example.com,b@example.com` puts these addresses on the allowlist as admins at every start, so the first admin can always get in. Without it, only people already on the list can sign in (the server logs a warning).
- Everyone else is added by an admin in **🛠 Admin → Who may watch** (or `POST /api/admin/allowlist`), as a viewer or an admin. Removing someone signs them out everywhere.
- A sign-in from an address that is not on the list lands on a "Not on the list" page and is recorded under **Refused sign-ins** with the address and provider, so the admin can allow it with one click; the person then signs in again.
- Only verified addresses count: an identity whose provider says the e-mail is not verified is refused.
- The login page shows only the buttons for the providers that are switched on (`GET /auth/providers`).

## Switching a running server from the token to Google and Apple

1. Set the provider settings above and `ADMIN_EMAILS` (on Fly.io, as secrets: see [deploy.md](deploy.md)), and `AUTH_MODE=oidc`. The token cookie stops working, which signs the token holders out.
2. Sign in as one of the `ADMIN_EMAILS`. Nothing else changes: the world, the model settings and the history live in the database, not in the sign-in mode.
3. Check the whole path once with a second account:
   - sign in with an address that is not on the list: it lands on "Not on the list";
   - as the admin, open **🛠 Admin → Who may watch**: the attempt is listed; press **Allow**;
   - the second account signs in again and watches the same world;
   - the admin changes the speed: both pages show it.
4. `curl -i <PUBLIC_ORIGIN>/api/stream` without a cookie answers `401`.

The sign-in pages are rate-limited per address. Behind a proxy that sets `Fly-Client-IP` or `X-Forwarded-For`, set `TRUST_PROXY=true` so the limit applies to the visitor rather than the proxy; never set it otherwise, or anyone could pick their own address.

## What is tested where

`server/crates/server/tests/auth.rs` runs the whole flow against a fake OpenID provider: an allowlisted Google sign-in creates a user and a session; an unknown address is refused, recorded and can then be allowed; Apple's `form_post` callback with its string `email_verified` and first-sign-in name; a bad state, nonce or audience is rejected; removing someone from the list ends their sessions; cross-site requests to admin routes are rejected; the sign-in pages are rate-limited. A real Google and Apple account can only be tried against real credentials, which is step 3 above.
