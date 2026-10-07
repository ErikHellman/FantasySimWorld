# Signing in

The server has three modes:

- `AUTH_MODE=off`: no sign-in; everyone is an admin. For local development only: the server refuses to start unless it listens on a loopback address (the default becomes `127.0.0.1:8080`) and `PUBLIC_ORIGIN` is `http://localhost` or a loopback IP, so a deployment can never run open.
- `AUTH_MODE=token` (the default): one shared secret in `AUTH_TOKEN` (16+ characters). Whoever opens `/auth/token` and types it gets a cookie and is an admin. Good for local work and a first deployment.
- `AUTH_MODE=oidc`: people sign in with an account they already have (Google, Apple, Microsoft, LinkedIn, GitHub or Discord), and only e-mail addresses on the allowlist get in.

With `token` and `oidc`, people who do not sign in may also watch as guests: a few at a time, for 15 minutes, without chat (see [Guests](#guests) below).

This page is about the last one. Set `PUBLIC_ORIGIN` to the address people type (for example `https://village.example.com`). Every provider sends people back to a redirect URI built from it, and the URI you register with the provider must match it exactly: same scheme, host, port and path, and no trailing slash.

## The providers

Pick one or more, follow each one's guide, and list them in `AUTH_PROVIDERS`, separated by commas. The login page shows one button per provider, in the order you list them.

```bash
AUTH_MODE=oidc
AUTH_PROVIDERS=google,microsoft,github
ADMIN_EMAILS=you@example.com
```

| Provider | `AUTH_PROVIDERS` name | Variables | Redirect URI to register | Guide |
| --- | --- | --- | --- | --- |
| Google | `google` | `GOOGLE_CLIENT_ID`, `GOOGLE_CLIENT_SECRET` | `<PUBLIC_ORIGIN>/auth/google/callback` | [google.md](sign-in/google.md) |
| Apple | `apple` | `APPLE_TEAM_ID`, `APPLE_KEY_ID`, `APPLE_SERVICES_ID`, `APPLE_PRIVATE_KEY` | `<PUBLIC_ORIGIN>/auth/apple/callback` | [apple.md](sign-in/apple.md) |
| Microsoft | `microsoft` | `MICROSOFT_CLIENT_ID`, `MICROSOFT_CLIENT_SECRET`, `MICROSOFT_TENANT` (optional) | `<PUBLIC_ORIGIN>/auth/microsoft/callback` | [microsoft.md](sign-in/microsoft.md) |
| LinkedIn | `linkedin` | `LINKEDIN_CLIENT_ID`, `LINKEDIN_CLIENT_SECRET` | `<PUBLIC_ORIGIN>/auth/linkedin/callback` | [linkedin.md](sign-in/linkedin.md) |
| GitHub | `github` | `GITHUB_CLIENT_ID`, `GITHUB_CLIENT_SECRET` | `<PUBLIC_ORIGIN>/auth/github/callback` | [github.md](sign-in/github.md) |
| Discord | `discord` | `DISCORD_CLIENT_ID`, `DISCORD_CLIENT_SECRET` | `<PUBLIC_ORIGIN>/auth/discord/callback` | [discord.md](sign-in/discord.md) |

The server checks the settings when it starts. A provider listed in `AUTH_PROVIDERS` without its variables, an unknown name, or a name given twice stops it with a message naming what is missing. Settings for providers that are not listed are ignored, so you can keep them while a provider is switched off.

Facebook and X (Twitter) are not offered. Neither says whether the e-mail address it gives out has been verified, and without that the allowlist could be fooled (see the next section).

## How people are matched to the allowlist

The allowlist holds e-mail addresses, not accounts. When someone signs in with a provider for the first time, the server asks the provider for their e-mail address and lets them in only if all of these hold:

1. The provider gave an address. People whose account has none, such as a Microsoft or Discord account made with only a phone number, are refused as "no e-mail".
2. The provider vouches that the person owns that address. Otherwise they are refused as "e-mail not verified". Each guide says what counts as verified for that provider.
3. The address is on the allowlist. Otherwise they are refused as "not on the list".

The second point is what keeps the allowlist safe. Because accounts are matched by address, a provider that handed out addresses nobody had confirmed would let a stranger type in an admin's address and sign in as them. That is why some providers need an extra setting in their guide (Microsoft's `xms_edov` claim), and why GitHub uses only the primary address.

After the first sign-in the provider's account is linked to the person. Later sign-ins with that provider find them by the link, even if their address at the provider has changed since. One person may use several providers: each is linked the first time it is used with an address that is already on the list. If someone's address differs between providers (say a work address on Microsoft and a private one on GitHub), that is two people as far as the allowlist goes, and each address has to be allowed.

## The allowlist

- `ADMIN_EMAILS=a@example.com,b@example.com` puts these addresses on the allowlist as admins at every start, so the first admin can always get in. Without it, only people already on the list can sign in, and the server logs a warning.
- Everyone else is added by an admin in **🛠 Admin → Who may watch** (or `POST /api/admin/allowlist`), as a viewer or an admin. Removing someone signs them out everywhere.
- A sign-in from an address that is not on the list lands on a "Not on the list" page. It is recorded under **Refused sign-ins** with the address, the provider and the reason, so the admin can allow it with one click; the person then signs in again.
- The login page shows only the buttons for the providers that are switched on (`GET /auth/providers`).

## Switching a running server from the token to accounts

1. Set up the providers you want (each guide ends with the variables to set), plus `ADMIN_EMAILS` and `AUTH_MODE=oidc`. On Fly.io these are secrets: see [deploy.md](deploy.md). The token cookie stops working, which signs the token holders out.
2. Sign in as one of the `ADMIN_EMAILS`. Nothing else changes: the world, the model settings and the history live in the database, not in the sign-in mode.
3. Check the whole path once for each provider, with a second account:
   - sign in with an address that is not on the list: it lands on "Not on the list";
   - as the admin, open **🛠 Admin → Who may watch**: the attempt is listed with the provider's name; press **Allow**;
   - the second account signs in again and watches the same world;
   - the admin changes the speed: both pages show it.
4. `curl -i <PUBLIC_ORIGIN>/api/stream` without a cookie (neither a session nor a guest pass) answers `401`.

The sign-in pages are rate-limited per address. Behind a proxy that sets `Fly-Client-IP` or `X-Forwarded-For`, set `TRUST_PROXY=true` so the limit applies to the visitor rather than the proxy. Never set it otherwise, or anyone could pick their own address.

## Guests

`POST /auth/guest` hands out a guest seat: a pass in an HttpOnly cookie (`__Host-guest` over https, `guest` over http) that lasts 15 minutes and lets its holder into the routes that only watch (`/api/stream`, `/api/me`, `/api/watch`, `/api/characters/…`, `/api/chronicle` and `/api/logout`). Everything else under `/api` answers `403` to a guest, and the chat treats them as having no identity at all.

- How many guests may watch at once is `GUEST_STREAMS`, fixed when the server is built (25 by default, `0` turns guests off). See the [README](../README.md#guests).
- When the seats are full, the answer is a ticket and a place in line instead of a cookie. The page asks again with the ticket once a minute; an earlier ask gets `429` with `Retry-After`, and the place is kept.
- The route is behind the same per-address rate limit and the same cross-site check (JSON only, same `Origin`) as the rest.
- A guest is never on the allowlist, and signing in wins over a guest pass still in the browser.

## When a sign-in fails

| What the person sees | Likely cause |
| --- | --- |
| The provider shows an error about the redirect URI before they can sign in | The URI registered with the provider does not match `<PUBLIC_ORIGIN>/auth/<provider>/callback` exactly. Check `http` against `https`, the port and trailing slashes. |
| "Sign-in is unavailable right now." | The server could not reach the provider's discovery document, for example on a network outage. The server log has the reason. |
| "The provider's answer could not be verified." | The code exchange or the token check failed: most often a wrong or expired client secret. The log says which. |
| "This sign-in has expired or was already used." | More than ten minutes passed on the provider's page, the server restarted in between, or the browser blocked the flow cookie. |
| "Not on the list" | See **Refused sign-ins** in the admin panel. Its reason says whether the address was missing, unverified or simply not allowed. |

## How the sign-in works

Every provider uses the authorisation-code flow, run entirely on the server:

1. `/auth/<provider>/start` remembers a random `state`, and for most providers a `nonce` and a PKCE verifier, under a short-lived cookie.
2. It sends the browser to the provider.
3. The provider sends the browser back to `/auth/<provider>/callback`.
4. The server checks `state`, then trades the code for tokens using the client secret.

For Google, Apple, Microsoft and LinkedIn the answer includes a signed id_token. The server checks its signature against the provider's published keys, along with the issuer, the audience (your client ID), the expiry and the nonce. GitHub and Discord have no id_token, so the server asks their API who signed in instead. The browser never sees a token or a secret.

## What is tested where

`server/crates/server/tests/auth.rs` runs the whole flow against fake providers:

- an allowlisted sign-in creates a user and a session
- an unknown address is refused, recorded and can then be allowed
- Apple's `form_post` callback, with its string `email_verified` and the name sent only at first sign-in
- Microsoft accepts personal accounts and work accounts with `xms_edov`, and refuses the rest
- LinkedIn signs in without PKCE and without a nonce
- GitHub uses the verified primary address
- Discord refuses unverified addresses
- a bad state, nonce, issuer or audience is rejected
- removing someone from the list ends their sessions
- cross-site requests to admin routes are rejected
- the sign-in pages are rate-limited

`server/crates/server/tests/http.rs` covers guests: a seat, the stream and the `403`s; the line and its `429`; the stream ending when the pass runs out or another tab takes over; and guests being off without seats or without sign-in. The seat-keeping itself is unit-tested in `server/crates/server/src/guests.rs`.

A real account at each provider can only be tried against real credentials, which is step 3 above.
