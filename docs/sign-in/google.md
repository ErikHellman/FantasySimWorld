# Signing in with Google

Anyone with a Google account (Gmail or Google Workspace) can sign in. It costs nothing. This guide assumes you have read [the overview](../sign-in.md) and set `PUBLIC_ORIGIN`.

## What you need

- A Google account to own the project.
- The address people will use, for example `https://village.example.com`. Google allows plain `http` only for `localhost`.

## 1. Create a project and the consent screen

1. Open the [Google Cloud console](https://console.cloud.google.com/), and create a project with the project picker at the top (or pick an existing one).
2. Go to **Google Auth Platform** (search for it, or open [console.cloud.google.com/auth](https://console.cloud.google.com/auth/overview)) and press **Get started**.
3. Fill in:
   - **App name**: what people see on the consent screen, such as "Fantasy Sim World".
   - **User support email**: your address.
   - **Audience**: **External**, so people outside your own Workspace organisation can sign in. Pick **Internal** only if everyone who will ever watch has an account in your Workspace.
   - **Contact information**: your address.
4. Agree to the user data policy and press **Create**.

## 2. Create the client

1. In **Google Auth Platform → Clients**, press **Create client**.
2. **Application type**: **Web application**. Give it a name, such as "Village server".
3. Leave **Authorized JavaScript origins** empty; the server does the sign-in, not the page.
4. Under **Authorized redirect URIs** add:

   ```text
   https://village.example.com/auth/google/callback
   ```

   that is, `<PUBLIC_ORIGIN>/auth/google/callback`. To try it locally as well, add `http://localhost:8080/auth/google/callback`.
5. Press **Create**. Copy the **Client ID** (it ends in `.apps.googleusercontent.com`) and the **Client secret**. Newer clients show the secret only now, so save it somewhere safe. If you lose it, add a new secret to the client and delete the old one.

## 3. Scopes and publishing

The server asks for `openid`, `email` and `profile`. These are not sensitive scopes, so Google needs no review and you need not add them under **Data Access**.

While the app's **Audience** says **Testing**, only the test users listed there can sign in at all; anyone else gets an error from Google before they reach the allowlist. Either add the people you invite as test users, or press **Publish app** under **Audience** to let anyone reach the allowlist check. Because only these basic scopes are used, publishing needs no verification by Google. Google may still show the app's name without a "verified" badge, which is fine.

## 4. Give the server the client

```bash
AUTH_MODE=oidc
AUTH_PROVIDERS=google
GOOGLE_CLIENT_ID=1234567890-abc.apps.googleusercontent.com
GOOGLE_CLIENT_SECRET=GOCSPX-…
ADMIN_EMAILS=you@example.com
```

On Fly.io:

```bash
fly secrets set AUTH_MODE=oidc AUTH_PROVIDERS=google ADMIN_EMAILS=you@example.com GOOGLE_CLIENT_ID=… GOOGLE_CLIENT_SECRET=…
```

## Which address counts

The address of the Google account, if Google says it is verified (`email_verified`). Gmail addresses always are. A Google account made with another provider's address is verified once its owner has confirmed it.

## Trying it locally

With the redirect URI for `localhost` registered, run the server with `PUBLIC_ORIGIN=http://localhost:8080` and the variables above, and open `http://localhost:8080/login`.

## Things that go wrong

- **"Error 400: redirect_uri_mismatch"**: the URI in step 2 differs from `<PUBLIC_ORIGIN>/auth/google/callback`. Google shows the URI it received; copy that one in.
- **"Access blocked: … has not completed the Google verification process"**: the app is in **Testing** and the account is not a test user. See step 3.
- **"The provider's answer could not be verified."** after a secret change: the server still has the old secret. Set the new one and restart.
