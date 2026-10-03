# Signing in with Microsoft

People can sign in with a personal Microsoft account (Outlook.com, Hotmail, Live, Xbox), with a work or school account from an organisation that uses Microsoft Entra ID (Microsoft 365), or with both. It costs nothing. This guide assumes you have read [the overview](../sign-in.md) and set `PUBLIC_ORIGIN`.

## What you need

- A Microsoft account that can register applications in a Microsoft Entra tenant. Any personal Microsoft account can make a free one when it first opens the Entra admin center. A work account may need its administrator's permission.
- The address people will use. Microsoft allows plain `http` only for `localhost`.

## 1. Register the application

1. Open the [Microsoft Entra admin center](https://entra.microsoft.com/) and go to **Entra ID → App registrations** (in some layouts **Identity → Applications → App registrations**), then **New registration**. The same page is in the Azure portal under **App registrations**.
2. **Name**: what people see when they sign in, such as "Fantasy Sim World".
3. **Supported account types**: who may sign in at all. The allowlist still decides who gets in. Choose the row in the table below, and note the matching `MICROSOFT_TENANT`:

   | Supported account types | Who can sign in | `MICROSOFT_TENANT` |
   | --- | --- | --- |
   | Accounts in any organizational directory and personal Microsoft accounts | Everyone with a Microsoft account | `common` (the default) |
   | Personal Microsoft accounts only | Outlook.com, Hotmail, Xbox and other personal accounts | `consumers` |
   | Accounts in any organizational directory (Multitenant) | Work and school accounts of any organisation | `organizations` |
   | Accounts in this organizational directory only (Single tenant) | Only your organisation | your **Directory (tenant) ID**, or a domain of it such as `contoso.onmicrosoft.com` |

4. **Redirect URI**: platform **Web**, address `<PUBLIC_ORIGIN>/auth/microsoft/callback`, such as `https://village.example.com/auth/microsoft/callback`.
5. Press **Register**. The overview page shows the **Application (client) ID**, which is `MICROSOFT_CLIENT_ID`. It also shows the **Directory (tenant) ID**, which you need only for a single-tenant registration.

To try it locally too, open **Authentication**, and under the **Web** platform add `http://localhost:8080/auth/microsoft/callback`.

## 2. A client secret

1. Open **Certificates & secrets → Client secrets → New client secret**.
2. Give it a description and an expiry. Microsoft allows at most 24 months.
3. Copy the **Value** column, not the **Secret ID**, straight away. It is shown only once. This is `MICROSOFT_CLIENT_SECRET`.

**Microsoft secrets expire.** When it does, every Microsoft sign-in fails with "The provider's answer could not be verified." Put the expiry date in your calendar. Before it, make a second secret, set it on the server, restart, and then delete the old one.

## 3. Let work and school addresses count

This step matters only if work or school accounts may sign in (`common`, `organizations` or a single tenant).

A work account's e-mail address is whatever its organisation's administrator typed in. On its own, Microsoft does not promise that the organisation owns that address, so anyone running their own Microsoft tenant could make an account claiming to be your admin. (This trick is known as "nOAuth".) The server therefore accepts a work or school address only when Microsoft adds the `xms_edov` claim ("e-mail domain owner verified"). The claim means the organisation has proven to Microsoft that it owns the address's domain. Microsoft sends it only when asked:

1. Open **Token configuration → Add optional claim**.
2. Token type **ID**. Tick **email** and **xms_edov**, then press **Add**.
3. If Microsoft asks to turn on the Microsoft Graph `email` permission, tick it and confirm.

Without this step, personal accounts still work, but every work or school account is refused as "e-mail not verified".

The server asks only for `openid`, `email` and `profile`, which need no administrator consent. **API permissions** may also list `User.Read`, which you can leave or remove.

## 4. Give the server the settings

```bash
AUTH_MODE=oidc
AUTH_PROVIDERS=microsoft
MICROSOFT_CLIENT_ID=00000000-0000-0000-0000-000000000000
MICROSOFT_CLIENT_SECRET=…
MICROSOFT_TENANT=common          # optional; see the table in step 1
ADMIN_EMAILS=you@example.com
```

On Fly.io:

```bash
fly secrets set MICROSOFT_CLIENT_ID=… MICROSOFT_CLIENT_SECRET=…
```

`MICROSOFT_TENANT` is not secret and may go under `[env]` in `fly.toml` instead.

## Which address counts

The `email` claim of the account, when:

- the account is a personal Microsoft account, whose address Microsoft has confirmed; or
- the account is a work or school account and the `xms_edov` claim is true (step 3).

Personal accounts made with only a phone number have no address and are refused as "no e-mail". So are work accounts whose administrator never filled in their e-mail.

## Trying it locally

Register `http://localhost:8080/auth/microsoft/callback` as in step 1, run the server with `PUBLIC_ORIGIN=http://localhost:8080` and the variables above, and open `http://localhost:8080/login`.

## Things that go wrong

- **AADSTS50011, "The redirect URI … does not match"**: the URI under **Authentication** differs from `<PUBLIC_ORIGIN>/auth/microsoft/callback`.
- **AADSTS50194 or AADSTS9002331, about multi-tenant or personal accounts**: `MICROSOFT_TENANT` does not fit the **Supported account types**. A "personal accounts only" registration needs `consumers`; a single-tenant one needs the tenant ID.
- **AADSTS7000215 or AADSTS7000222, "invalid client secret" or "expired"**: the secret is wrong (perhaps the Secret ID instead of the Value), or it has expired. See step 2.
- **Work accounts are refused as "e-mail not verified"**: step 3 is missing, or the organisation has not verified its domain with Microsoft.
