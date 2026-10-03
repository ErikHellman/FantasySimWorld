# Signing in with GitHub

Anyone with a GitHub account can sign in. It costs nothing. This guide assumes you have read [the overview](../sign-in.md) and set `PUBLIC_ORIGIN`.

## What you need

- A GitHub account, or an organisation you administer, to own the app.

Use an **OAuth App**, not a GitHub App. GitHub Apps can sign people in too, but they hand out e-mail addresses only with an extra permission, and they are made for acting on repositories, which the village never does.

## 1. Register the OAuth App

1. Open **Settings → Developer settings → OAuth Apps** ([github.com/settings/developers](https://github.com/settings/developers)) and press **New OAuth App**. For an organisation, open the organisation's **Settings → Developer settings → OAuth Apps** instead.
2. Fill in:
   - **Application name**: what people see when they sign in.
   - **Homepage URL**: `PUBLIC_ORIGIN`, such as `https://village.example.com`.
   - **Authorization callback URL**: `<PUBLIC_ORIGIN>/auth/github/callback`, such as `https://village.example.com/auth/github/callback`.
   - Leave **Enable Device Flow** off.
3. Press **Register application**. The page shows the **Client ID**.
4. Press **Generate a new client secret** and copy it now. GitHub shows it only once.

An OAuth App has exactly one callback URL. To try sign-in locally as well, register a second OAuth App with the callback `http://localhost:8080/auth/github/callback`, and use its ID and secret on your own machine.

## 2. Give the server the settings

```bash
AUTH_MODE=oidc
AUTH_PROVIDERS=github
GITHUB_CLIENT_ID=Ov23li…
GITHUB_CLIENT_SECRET=…
ADMIN_EMAILS=you@example.com
```

On Fly.io:

```bash
fly secrets set GITHUB_CLIENT_ID=… GITHUB_CLIENT_SECRET=…
```

## Which address counts

The **primary** address of the GitHub account, and only if GitHub has verified it. The server asks for the `read:user` and `user:email` scopes, so it sees the address even when the person keeps it private on their profile. It never shows it to anyone but the admin.

Other verified addresses on the account do not count. If someone's primary address is not the one you allowed, either allow their primary address, or ask them to make the allowed address primary under **Settings → Emails** on GitHub. An unverified primary address is refused as "e-mail not verified".

The display name is the account's name, or its login when the name is empty.

## How GitHub differs from the others

GitHub does not speak OpenID Connect, so there is no signed id_token. After the code exchange (with PKCE and the client secret), the server asks GitHub's API who the token belongs to (`/user`) and which addresses are verified (`/user/emails`). The access token is used for those two calls and then forgotten.

## Things that go wrong

- **"The redirect_uri is not associated with this application"** on GitHub's page: the callback URL in step 1 differs from `<PUBLIC_ORIGIN>/auth/github/callback`. A GitHub App's callback will not do either; it must be an OAuth App.
- **"The provider's answer could not be verified."**: usually a wrong or regenerated client secret. The server log has GitHub's answer.
- **Someone is refused although their address is on the list**: it is not their primary address on GitHub. See above.
- **"This application has been suspended"** or a rate-limit message: GitHub has flagged the app or the server's address. Check the app's page on GitHub.
