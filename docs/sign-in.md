# Access

Everyone who opens the page watches the village. There are no accounts: the browser asks the server for a guest seat and gets one, or a place in line when every seat is taken. Guests look around, follow villagers and talk in the chat under a name of their choosing. Only running the world (the speed, new worlds, imports and exports, the billboards, the model test, the server's health) needs an admin.

The server has two modes:

- `AUTH_MODE=token` (the default): one shared secret in `AUTH_TOKEN` (16+ characters). Whoever opens `/auth/token` and types it gets a cookie and is an admin; everyone else is a guest. The link to the form is at the bottom of ⚙ Settings.
- `AUTH_MODE=off`: no token; everyone is an admin, and nobody needs a seat. For local development only: the server refuses to start unless it listens on a loopback address (the default becomes `127.0.0.1:8080`) and `PUBLIC_ORIGIN` is `http://localhost` or a loopback IP, so a deployment can never run open.

## Seats and the line

`POST /auth/guest` hands out a seat: a pass in an HttpOnly cookie (`__Host-guest` over https, `guest` over http) that lets its holder into every `/api` route but `/api/admin`, which answers `403` to a guest.

- **How many.** `MAX_VIEWERS` (25 unless set) guests watch at once; `0` means no cap. The seats live in memory; a restart empties them, and the browsers simply ask again.
- **How long.** For as long as the guest watches. A seat is shared by the browser's tabs (they share the cookie) and holds one stream: a newer tab takes it over from an older one, which is told `guest_end` with the reason `elsewhere`. A seat with no open stream is given up after a minute (three minutes for a page that is still loading the valley's models).
- **The line.** When every seat is taken, the answer is a ticket and a place in line instead of a cookie. The page asks again with the ticket once a minute; an earlier ask gets `429` with `Retry-After`, and the place is kept. A seat that comes free goes to whoever is first in line. A place nobody asks about for three minutes is given up.
- **Leaving.** **Leave** in ⚙ Settings (`POST /api/logout`) gives the seat to the next person straight away.
- The route is behind the same per-address rate limit and the same cross-site check (JSON only, same `Origin`) as the rest.

## Chat names

Each guest pass is one person in the chat: all of a browser's tabs share one name, chosen in ⚙ Settings and kept in that browser. Admins, who all hold the same token, are one person per tab. See the [README](../README.md#chat).

## Security

- Mutating `/api` requests must carry a JSON body and an `Origin` equal to `PUBLIC_ORIGIN` (or a same-origin `Sec-Fetch-Site`), so a form on another site cannot act with a visitor's cookie.
- `/auth/token` and `/auth/guest` are rate-limited per address. Behind a proxy that sets `Fly-Client-IP` or `X-Forwarded-For`, set `TRUST_PROXY=true` so the limit applies to the visitor rather than the proxy. Never set it otherwise, or anyone could pick their own address.
- The token is compared in constant time and never appears in a URL: the form posts it once and the cookie carries it afterwards. `/api/*` also accepts it as `Authorization: Bearer`.

## What is tested where

`server/crates/server/tests/http.rs` covers the token (the form, the cookie, the bearer header, the rate limit, the cross-site refusals), sign-in off, and guests: a seat, the stream, the chat name shared by a browser's tabs, the `403` on admin routes, the line and its `429`, a newer tab taking the stream over, no cap, and the admin's token winning over a guest cookie. The seat-keeping itself is unit-tested in `server/crates/server/src/guests.rs`.
