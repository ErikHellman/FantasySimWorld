# Signing in with LinkedIn

Anyone with a LinkedIn account can sign in. It costs nothing. This guide assumes you have read [the overview](../sign-in.md) and set `PUBLIC_ORIGIN`.

## What you need

- A LinkedIn account to own the app.
- A **LinkedIn Page** for the app to belong to. LinkedIn requires every app to be tied to a company Page, and you can create one for the village in a minute (**For Business → Create a Company Page**). You must be an admin of the Page.

## 1. Create the app

1. Open the [LinkedIn developer portal](https://www.linkedin.com/developers/apps) and press **Create app**.
2. Fill in:
   - **App name**: what people see when they sign in.
   - **LinkedIn Page**: the Page from above.
   - **Privacy policy URL** (optional) and **App logo** (required; any square image).
3. Agree to the legal terms and press **Create app**.

## 2. Turn on "Sign In with LinkedIn using OpenID Connect"

1. Open the app's **Products** tab.
2. Find **Sign In with LinkedIn using OpenID Connect** and press **Request access**, then agree to the terms. It is granted at once.
3. On the **Auth** tab, under **OAuth 2.0 scopes**, you should now see `openid`, `profile` and `email`. Those are the ones the server asks for.

## 3. The redirect URL and the credentials

1. On the **Auth** tab, under **OAuth 2.0 settings → Authorized redirect URLs for your app**, press the pencil and add `<PUBLIC_ORIGIN>/auth/linkedin/callback`, such as `https://village.example.com/auth/linkedin/callback`. Press **Update**.
2. To try it locally as well, add `http://localhost:8080/auth/linkedin/callback`.
3. At the top of the **Auth** tab, copy the **Client ID** and the **Primary Client Secret** (press the eye to show it).

## 4. Give the server the settings

```bash
AUTH_MODE=oidc
AUTH_PROVIDERS=linkedin
LINKEDIN_CLIENT_ID=86abcdefghijkl
LINKEDIN_CLIENT_SECRET=…
ADMIN_EMAILS=you@example.com
```

On Fly.io:

```bash
fly secrets set LINKEDIN_CLIENT_ID=… LINKEDIN_CLIENT_SECRET=…
```

## Which address counts

The primary e-mail address of the LinkedIn account, when LinkedIn says it is verified (`email_verified`). LinkedIn describes both as optional. A sign-in without an address is refused as "no e-mail", and one that is not marked verified as "e-mail not verified". The person should confirm their primary address in LinkedIn's settings (**Settings → Sign in & security → Email addresses**) and try again.

## How LinkedIn differs from the others

- **No PKCE.** LinkedIn accepts PKCE only from native apps that it has approved, so the server does not use it with LinkedIn. The code is still protected: `state` ties it to the browser that started the sign-in, and only the server, with its secret, can trade it for a token.
- **The nonce may be missing.** LinkedIn does not promise to repeat the nonce in its id_token. The server checks it when it is there and accepts its absence, for the same reason.
- **Two issuer spellings.** LinkedIn's documents name both `https://www.linkedin.com` and `https://www.linkedin.com/oauth`, and the server accepts either.

## Things that go wrong

- **"The redirect_uri does not match the registered value"**: the URL in step 3 differs from `<PUBLIC_ORIGIN>/auth/linkedin/callback`.
- **"unauthorized_scope_error"** on LinkedIn's page: the product in step 2 has not been added, so the app may not ask for `openid`, `profile` and `email`.
- **Everyone is refused as "e-mail not verified"**: the people concerned have not confirmed their primary address with LinkedIn.
- **The secret stopped working**: someone generated a new client secret on the **Auth** tab. Set the new one on the server and restart.
