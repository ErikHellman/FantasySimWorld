# Signing in with Discord

Anyone with a Discord account and a verified e-mail address can sign in. It costs nothing. This guide assumes you have read [the overview](../sign-in.md) and set `PUBLIC_ORIGIN`.

## What you need

- A Discord account to own the application. Discord asks for a verified e-mail address (and sometimes two-factor authentication) before it lets you create one.

The server does not need a bot, a Discord server (guild) or any permission beyond reading who signed in.

## 1. Create the application

1. Open the [Discord developer portal](https://discord.com/developers/applications) and press **New Application**.
2. Give it a name (people see it when they sign in), agree to the terms and press **Create**.
3. Optionally, on **General Information**, add an icon and a description.

## 2. The redirect and the credentials

1. Open the **OAuth2** page.
2. Under **Redirects**, press **Add Redirect** and enter `<PUBLIC_ORIGIN>/auth/discord/callback`, such as `https://village.example.com/auth/discord/callback`. To try it locally as well, add `http://localhost:8080/auth/discord/callback`. Press **Save Changes**.
3. Leave **Public Client** off. The server keeps the secret, and Discord should insist on it.
4. Copy the **Client ID**. Press **Reset Secret** to make a **Client Secret**, and copy it now; Discord shows it only once.

You do not need the **OAuth2 URL Generator**. The server builds the sign-in link itself, with the `identify` and `email` scopes.

## 3. Give the server the settings

```bash
AUTH_MODE=oidc
AUTH_PROVIDERS=discord
DISCORD_CLIENT_ID=1234567890123456789
DISCORD_CLIENT_SECRET=…
ADMIN_EMAILS=you@example.com
```

On Fly.io:

```bash
fly secrets set DISCORD_CLIENT_ID=… DISCORD_CLIENT_SECRET=…
```

## Which address counts

The account's e-mail address, when Discord says it is verified. Discord accounts made with only a phone number have no address and are refused as "no e-mail". Accounts that never confirmed their address are refused as "e-mail not verified". The person can fix it under **User Settings → My Account** in Discord and try again.

The display name is the person's display name on Discord, or their username when they have none.

## How Discord differs from the others

Discord does not speak OpenID Connect, so there is no signed id_token. After the code exchange (with PKCE and the client secret), the server asks Discord's API who the token belongs to (`/users/@me`). The answer includes the address and whether it is verified. The access token is used for that one call and then forgotten.

## Things that go wrong

- **"Invalid OAuth2 redirect_uri"** on Discord's page: the redirect in step 2 differs from `<PUBLIC_ORIGIN>/auth/discord/callback`, or **Save Changes** was not pressed.
- **"The provider's answer could not be verified."**: a wrong or reset client secret. Set the current one on the server and restart.
- **Someone is refused as "e-mail not verified"**: they have not confirmed their address with Discord. See above.
