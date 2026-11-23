# Roynit ID

The account Worker is deployed at `https://roynit.dangz.org/api/account`. Roy Music uses that URL by default. Other first-party Roynit apps can authenticate users against the same account database. Keep each app’s data in its own authenticated endpoints.

## Local testing

From `accounts/`:

```sh
npx wrangler d1 migrations apply roynit-accounts --local
npx wrangler dev --local --ip 127.0.0.1 --port 8790
```

Add `REGISTER_LIMIT=200` to the git-ignored `accounts/.dev.vars` so the smoke suite can create enough test accounts (the default is 8 per 10 minutes per address). Social features (friends, presence, messages) need migration `0004_social.sql`; apply it with `npx wrangler d1 migrations apply roynit-accounts --remote` before deploying.

Run `node --test accounts/smoke.mjs` from the repository root. For the desktop app, set `ROYNIT_ACCOUNT_ENDPOINT=http://127.0.0.1:8790/api/account` before launch and open **Roynit ID** from the top-left menu. Use a separate desktop data directory with `--data-dir` when testing.

## Enable Google sign-in

Create a **Web application** OAuth client in the Google Cloud project for Roynit ID. Add the following authorized redirect URI.

```text
https://roynit.dangz.org/api/account/google/callback
```

From `accounts/`, set both values as Cloudflare Worker secrets.

```sh
npx wrangler secret put GOOGLE_CLIENT_ID
npx wrangler secret put GOOGLE_CLIENT_SECRET
```

The client requests `openid email profile`. Check `/api/account/health` for `google: true`, then verify the browser flow with a test user approved on the Google consent screen. Google identities are keyed by Google subject and independent of email/password identities. Linking existing accounts is a separate flow.

## Link Google / YouTube and Spotify

Linking lets a Roynit ID (email + password or Google) unlock Settings → Accounts in the app without anyone creating their own client ID. The Worker runs the OAuth flow, keeps each refresh token encrypted (AES-GCM, bound to user and provider) and hands the app only short-lived access tokens.

1. Google: add `https://roynit.dangz.org/api/account/link/callback` as a second authorized redirect URI on the same Web client, and enable the **YouTube Data API v3**. The `youtube.readonly` scope is sensitive, so the consent screen needs Google verification before people outside your test users can link.
2. Spotify: create an app at developer.spotify.com, add the same redirect URI, and note that apps in development mode only allow users you add under User Management.
3. Set the secrets from `accounts/`, then apply the migrations and deploy:

```sh
node -e "console.log(require('crypto').randomBytes(32).toString('base64'))"   # use the output as LINK_KEY
npx wrangler secret put LINK_KEY
npx wrangler secret put SPOTIFY_CLIENT_ID
npx wrangler secret put SPOTIFY_CLIENT_SECRET
npx wrangler d1 migrations apply roynit-accounts --remote
npx wrangler deploy
```

`/health` reports `link: {google, spotify}`; the app disables a Link button until its provider is configured. Losing `LINK_KEY` only means people must link again. Endpoints: `GET /links`, `POST /link/<provider>/start`, `GET /link/callback` (browser redirect target), `POST /link/<provider>/token`, `DELETE /link/<provider>`.

### Local smoke test

`accounts/.dev.vars` (git-ignored) points the providers at the mock server that `smoke.mjs` starts on port 8791:

```text
LINK_KEY=<any 32 random bytes, base64>
GOOGLE_CLIENT_ID=test-google
GOOGLE_CLIENT_SECRET=test-google-secret
SPOTIFY_CLIENT_ID=test-spotify
SPOTIFY_CLIENT_SECRET=test-spotify-secret
OAUTH_TEST_BASE=http://127.0.0.1:8791
```

## Account and sync contract

- `POST /register` accepts `email`, `username` and `password`. `POST /login` accepts an email or username in `login` plus `password`. The response contains `account` and a bearer `token`. Email is an identifier in the password flow. Email verification and password recovery are pending.
- `GET /me` returns the current profile (`display_name`, `bio`, `avatar_url`, `created_at`, …). `PATCH /me` updates `display_name` (up to 48 characters) and/or `bio` (up to 160). `PUT /me/avatar` takes the raw image bytes (JPEG, PNG or WebP, under 150 KB, checked against the file signature) and `DELETE /me/avatar` removes it. `GET /avatar/<user id>?v=<version>` serves the picture publicly and cacheable; the version changes with every upload. Roy Music crops and shrinks pictures to 256 px before uploading. `POST /logout` revokes the current session. Sessions expire after 30 days. Roy Music stores its token using Windows DPAPI.
- `GET /music` returns only the signed-in user's snapshot. `PUT /music` accepts a `snapshot` and the expected `revision`. A stale revision returns HTTP 409. Merge the downloaded cloud library into the current device before resuming uploads.
- Music uploads are opt-in. The snapshot contains online track metadata and references for liked tracks, playlists and recent listening history. Local audio, filesystem paths, cookies, playback URLs, and provider credentials stay on the device. Listening history merged into the local library is available to the existing mix engine. Local library data stays on the device after sign-out.
- To deploy changes, apply D1 migrations remotely before running `npx wrangler deploy` from `accounts/`. The Worker owns only `/api/account/*` on the existing site.
