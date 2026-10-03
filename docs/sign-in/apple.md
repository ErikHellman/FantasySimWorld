# Signing in with Apple

Anyone with an Apple Account can sign in. This guide assumes you have read [the overview](../sign-in.md) and set `PUBLIC_ORIGIN`.

## What you need

- A membership of the [Apple Developer Program](https://developer.apple.com/programs/) (paid, yearly). Sign in with Apple cannot be set up without it.
- A real domain with `https`. Apple redirects only to an `https` address on a domain you register with it, so Apple sign-in can be tried only on the deployed server, never on `localhost`.

## 1. An App ID with Sign in with Apple

1. Open [Certificates, Identifiers & Profiles → Identifiers](https://developer.apple.com/account/resources/identifiers/list) and press **+**.
2. Choose **App IDs**, then **App**. Give it a description and a bundle ID such as `com.example.village`.
3. Under **Capabilities** tick **Sign in with Apple**, then **Continue** and **Register**.

If you already have an App ID, edit it and tick **Sign in with Apple** instead.

## 2. A Services ID (the client ID)

1. In **Identifiers**, press **+** again and choose **Services IDs**.
2. Give it a description (people see it on Apple's sign-in page) and an identifier such as `com.example.village.web`. This identifier is your client ID.
3. Register it, open it again, tick **Sign in with Apple** and press **Configure**:
   - **Primary App ID**: the App ID from step 1.
   - **Domains and Subdomains**: your domain without `https://`, such as `village.example.com`.
   - **Return URLs**: `<PUBLIC_ORIGIN>/auth/apple/callback`, such as `https://village.example.com/auth/apple/callback`.
4. Press **Next**, **Done**, **Continue** and **Save**.

## 3. A key to sign the client secret

Apple has no fixed client secret. The server makes one itself, a short-lived token signed with a key you create:

1. Open [Keys](https://developer.apple.com/account/resources/authkeys/list) and press **+**.
2. Name the key, tick **Sign in with Apple**, press **Configure** and pick the App ID from step 1.
3. Register it and **download the `.p8` file**. Apple lets you download it only once, so keep it safe.
4. Note the **Key ID** shown with the key, and your **Team ID** (top right of the developer site, under your name).

The server signs a new client secret every 30 days with this key. Nothing needs renewing by hand unless you revoke the key.

## 4. Give the server the settings

```bash
AUTH_MODE=oidc
AUTH_PROVIDERS=apple            # or google,apple and so on
APPLE_TEAM_ID=<10 characters>
APPLE_KEY_ID=<10 characters>
APPLE_SERVICES_ID=com.example.village.web
APPLE_PRIVATE_KEY="$(cat AuthKey_XXXXXXXXXX.p8)"
ADMIN_EMAILS=you@example.com
```

`APPLE_PRIVATE_KEY` is the whole content of the `.p8` file, `-----BEGIN PRIVATE KEY-----` lines included. Where a secret cannot hold line breaks, write `\n` in their place. The server refuses to start if the key cannot be read.

On Fly.io:

```bash
fly secrets set APPLE_TEAM_ID=… APPLE_KEY_ID=… APPLE_SERVICES_ID=com.example.village.web APPLE_PRIVATE_KEY="$(cat AuthKey_XXXXXXXXXX.p8)"
```

## Which address counts

The address Apple gives, which Apple always marks as verified. People who choose **Hide My Email** arrive with an `…@privaterelay.appleid.com` address. That is the address to allow; it shows up under **Refused sign-ins** after their first try.

Apple sends the person's name only the first time they sign in to your app. The server keeps it as their display name; if it is missing, the part of the address before the `@` is used.

## Things that go wrong

- **"invalid_client"** on Apple's page: the Services ID, the domain or the return URL in step 2 does not match. They must match `APPLE_SERVICES_ID` and `<PUBLIC_ORIGIN>` exactly.
- **"The provider's answer could not be verified."**: usually a wrong Team ID or Key ID, or a key made for another App ID. The server log has Apple's answer.
- **"This sign-in has expired or was already used."**: Apple posts its answer back to the server from its own page. That needs the flow cookie with `SameSite=None`, which the server sets only over `https`.
