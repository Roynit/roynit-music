import assert from "node:assert/strict";
import { createServer } from "node:http";
import test from "node:test";

const base = "http://127.0.0.1:8790/api/account";
// A Google account can belong to one Roynit ID only, so every run of this file uses a fresh one.
const runId = crypto.randomUUID();
async function request(path, method = "GET", body, token, expected = 200, extra = {}) {
  const headers = { ...extra };
  if (body !== undefined) headers["content-type"] = "application/json";
  if (token) headers.authorization = "Bearer " + token;
  const response = await fetch(base + path, { method, headers, body: body === undefined ? undefined : JSON.stringify(body) });
  const data = await response.json();
  assert.equal(response.status, expected, JSON.stringify(data));
  return data;
}

test("credentials, profile, private music sync, conflict and session revocation", async () => {
  const nonce = crypto.randomUUID().replaceAll("-", "").slice(0, 11);
  const password = "local-" + crypto.randomUUID();
  const alice = { email: nonce + "@example.test", username: "a_" + nonce, password };
  const bob = { email: "b_" + nonce + "@example.test", username: "b_" + nonce, password };
  const a = await request("/register", "POST", alice);
  const b = await request("/register", "POST", bob);
  assert.notEqual(a.account.id, b.account.id);
  assert.equal((await request("/login","POST",{login:alice.username,password})).account.id,a.account.id);
  assert.equal((await request("/login","POST",{login:alice.email,password})).account.id,a.account.id);
  await request("/login","POST",{login:alice.email,password:"incorrect-password"},undefined,401);
  assert.equal((await request("/me","PATCH",{display_name:"Roy Test"},a.token)).account.display_name,"Roy Test");
  const track = { id:"track", title:"Example", artist:"Artist", platform:"ytmusic", source:"https://music.youtube.com/watch?v=dQw4w9WgXcQ", local_path:"C:/local" };
  const snapshot = { tracks:{track}, playlists:{}, favorites:["track"], history:[{track_id:"track",started_at:123}] };
  assert.equal((await request("/music","PUT",{revision:0,snapshot},a.token)).revision,1);
  assert.equal((await request("/music","PUT",{revision:1,snapshot},a.token)).revision,2);
  await request("/music","PUT",{revision:1,snapshot},a.token,409);
  const saved = (await request("/music","GET",undefined,a.token)).snapshot;
  assert.equal(saved.history[0].started_at,123);
  assert.equal(saved.tracks.track.local_path,undefined);
  assert.equal((await request("/music","GET",undefined,b.token)).revision,0);
  await request("/logout","POST",undefined,a.token);
  await request("/me","GET",undefined,a.token,401);
});

test("profile details and avatar", async () => {
  const nonce = crypto.randomUUID().replaceAll("-", "").slice(0, 11);
  const password = "local-" + crypto.randomUUID();
  const a = await request("/register", "POST", { email: nonce + "@example.test", username: "p_" + nonce, password });
  const b = await request("/register", "POST", { email: "q_" + nonce + "@example.test", username: "q_" + nonce, password });
  assert.equal(a.account.avatar_url, null);
  assert.equal(a.account.bio, "");
  // Name and bio change independently; an empty update is refused.
  let me = (await request("/me", "PATCH", { bio: "  Late night   drives  " }, a.token)).account;
  assert.equal(me.bio, "Late night drives");
  assert.equal(me.display_name, "p_" + nonce);
  me = (await request("/me", "PATCH", { display_name: "Roy" }, a.token)).account;
  assert.equal(me.bio, "Late night drives");
  await request("/me", "PATCH", {}, a.token, 400);
  // Birthday: private to the owner, real dates only, and can be cleared.
  assert.equal(a.account.birthday, "");
  assert.equal((await request("/me", "PATCH", { birthday: "2000-02-29" }, a.token)).account.birthday, "2000-02-29");
  await request("/me", "PATCH", { birthday: "2001-02-29" }, a.token, 400);
  await request("/me", "PATCH", { birthday: "2999-01-01" }, a.token, 400);
  assert.equal((await request("/me", "PATCH", { birthday: "" }, a.token)).account.birthday, "");
  await request("/me", "PATCH", { bio: "x".repeat(161) }, a.token, 400);
  await request("/me", "PATCH", { display_name: "bad\nname" }, a.token, 400);

  const png = Buffer.from("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==", "base64");
  const put = (type, body, token) => fetch(base + "/me/avatar", { method: "PUT", headers: { "content-type": type, authorization: "Bearer " + token }, body });
  assert.equal((await put("image/png", Buffer.from("not an image"), a.token)).status, 400);
  assert.equal((await put("image/gif", png, a.token)).status, 400);
  assert.equal((await put("image/png", Buffer.alloc(160_000, 1), a.token)).status, 413);
  const stored = await (await put("image/png", png, a.token)).json();
  assert.match(stored.account.avatar_url, /\/avatar\/[0-9a-f-]{36}\?v=1$/);
  const image = await fetch(stored.account.avatar_url.replace("https://roynit.dangz.org/api/account", base));
  assert.equal(image.status, 200);
  assert.equal(image.headers.get("content-type"), "image/png");
  assert.match(image.headers.get("cache-control"), /immutable/);
  assert.deepEqual(Buffer.from(await image.arrayBuffer()), png);
  // A second upload bumps the version; one user's token cannot touch another's picture.
  const again = await (await put("image/png", png, a.token)).json();
  assert.match(again.account.avatar_url, /v=2$/);
  assert.equal((await request("/me", "GET", undefined, b.token)).account.avatar_url, null);
  assert.equal((await fetch(base + "/avatar/" + "0".repeat(8) + "-0000-0000-0000-" + "0".repeat(12))).status, 404);
  assert.equal((await fetch(base + "/avatar/..%2Fme")).status, 404);
  const removed = (await request("/me/avatar", "DELETE", undefined, a.token)).account;
  assert.equal(removed.avatar_url, null);
  assert.equal((await fetch(stored.account.avatar_url.replace("https://roynit.dangz.org/api/account", base))).status, 404);
});

// Needs the Worker started with the vars in accounts/README.md ("Local smoke test"): a mock provider on :8791.
test("linking Google and Spotify keeps refresh tokens on the server", async (t) => {
  const seen = [];
  const mock = createServer((req, res) => {
    let body = "";
    req.on("data", (chunk) => body += chunk);
    req.on("end", () => {
      const form = new URLSearchParams(body);
      seen.push({ url: req.url, form: Object.fromEntries(form) });
      const send = (status, value) => { res.writeHead(status, { "content-type": "application/json" }); res.end(JSON.stringify(value)); };
      if (req.url.endsWith("/token")) {
        if (form.get("grant_type") === "authorization_code") {
          return form.get("code") === "good" ? send(200, { access_token: "access-0", refresh_token: "refresh-A", expires_in: 3600 }) : send(400, { error: "invalid_grant" });
        }
        return mock.revoked || form.get("refresh_token") !== "refresh-A" ? send(400, { error: "invalid_grant" }) : send(200, { access_token: "access-1", expires_in: 1800 });
      }
      if (req.url === "/google/identity") return send(200, { email: "listener@example.test", sub: "google-sub-" + runId });
      if (req.url === "/spotify/identity") return send(200, { display_name: "Roy on Spotify" });
      send(404, {});
    });
  });
  await new Promise((resolve) => mock.listen(8791, "127.0.0.1", resolve));
  t.after(() => mock.close());
  const nonce = crypto.randomUUID().replaceAll("-", "").slice(0, 11);
  const user = await request("/register", "POST", { email: "l_" + nonce + "@example.test", username: "l_" + nonce, password: "local-" + crypto.randomUUID() });
  const other = await request("/register", "POST", { email: "m_" + nonce + "@example.test", username: "m_" + nonce, password: "local-" + crypto.randomUUID() });
  assert.deepEqual((await request("/health")).link, { google: true, spotify: true });
  await request("/link/google/start", "POST", undefined, undefined, 401);
  assert.deepEqual((await request("/links", "GET", undefined, user.token)).links, []);

  const start = await request("/link/google/start", "POST", undefined, user.token);
  const auth = new URL(start.url);
  assert.match(auth.searchParams.get("scope"), /youtube\.readonly/);
  assert.equal(auth.searchParams.get("access_type"), "offline");
  assert.equal(auth.searchParams.get("code_challenge_method"), "S256");
  const state = auth.searchParams.get("state");
  const callback = (code, value = state) => fetch(base + "/link/callback?state=" + value + "&code=" + code);
  assert.equal((await callback("bad")).status, 400);
  assert.equal((await callback("good")).status, 400, "the attempt is single use, even after a failed exchange");
  const again = new URL((await request("/link/google/start", "POST", undefined, user.token)).url).searchParams.get("state");
  assert.equal((await callback("good", again)).status, 200);
  assert.equal((await callback("good", again)).status, 400);
  const exchange = seen.find((call) => call.form.code === "good");
  assert.ok(exchange.form.code_verifier && exchange.form.client_secret, "PKCE verifier and client secret are sent by the server");

  const list = await request("/links", "GET", undefined, user.token);
  assert.deepEqual(list.links.map(({ provider, label }) => ({ provider, label })), [{ provider: "google", label: "listener@example.test" }]);
  assert.deepEqual((await request("/links", "GET", undefined, other.token)).links, []);
  // Linking Google also makes that Google account a way to sign in; it can only belong to one Roynit ID.
  assert.equal((await request("/me", "GET", undefined, user.token)).account.has_google, true);
  assert.equal((await request("/me", "GET", undefined, other.token)).account.has_google, false);
  const token = await request("/link/google/token", "POST", undefined, user.token);
  assert.equal(token.access_token, "access-1");
  assert.equal(seen.at(-1).form.refresh_token, "refresh-A", "the stored refresh token round-trips through encryption");
  await request("/link/google/token", "POST", undefined, other.token, 404);
  await request("/link/spotify/token", "POST", undefined, user.token, 404);

  const spotify = new URL((await request("/link/spotify/start", "POST", undefined, user.token)).url);
  assert.match(spotify.searchParams.get("scope"), /user-library-read/);
  assert.equal((await callback("good", spotify.searchParams.get("state"))).status, 200);
  assert.equal((await request("/links", "GET", undefined, user.token)).links.length, 2);
  assert.equal((await request("/link/spotify", "DELETE", undefined, user.token)).links.length, 1);

  mock.revoked = true;
  const revoked = await request("/link/google/token", "POST", undefined, user.token, 409);
  assert.match(revoked.error, /Link the account again/);
  assert.deepEqual((await request("/links", "GET", undefined, user.token)).links, []);
});

test("friends, presence and direct messages", async () => {
  const nonce = crypto.randomUUID().replaceAll("-", "").slice(0, 11);
  const password = "local-" + crypto.randomUUID();
  const make = (tag) => request("/register", "POST", { email: tag + nonce + "@example.test", username: tag + "_" + nonce, password });
  const [a, b, c] = [await make("fa"), await make("fb"), await make("fc")];
  const name = (user) => user.account.username;

  await request("/friends/request", "POST", { username: name(a) }, a.token, 400);
  await request("/friends/request", "POST", { username: "nobody_" + nonce }, a.token, 404);
  await request("/friends/request", "POST", { username: name(b) }, a.token);
  await request("/friends/request", "POST", { username: name(b) }, a.token, 409);
  assert.equal((await request("/social", "GET", undefined, a.token)).outgoing.length, 1);
  const incoming = (await request("/social", "GET", undefined, b.token)).incoming;
  assert.deepEqual(incoming.map((p) => p.id), [a.account.id]);
  // Nothing can be sent or read before the request is accepted.
  await request("/messages/" + a.account.id, "POST", { body: "hi" }, b.token, 403);
  await request("/friends/accept", "POST", { id: b.account.id }, a.token, 404);
  await request("/friends/accept", "POST", { id: a.account.id }, b.token);
  assert.equal((await request("/social", "GET", undefined, a.token)).friends[0].id, b.account.id);

  // Presence: offline until a heartbeat, then online with the track; invisible hides it again.
  assert.equal((await request("/social", "GET", undefined, a.token)).friends[0].online, false);
  const song = { title: "Genius", artist: "Ravyn Lenae", cover: "https://example.test/c.jpg", duration: 156, position: 54, playing: true };
  await request("/presence", "PUT", { listening: song }, b.token);
  let seen = (await request("/social", "GET", undefined, a.token)).friends[0];
  assert.equal(seen.online, true);
  assert.equal(seen.listening.title, "Genius");
  await request("/presence", "PUT", { invisible: true, listening: song }, b.token);
  seen = (await request("/social", "GET", undefined, a.token)).friends[0];
  assert.equal(seen.online, false);
  assert.equal(seen.listening, null);
  await request("/presence", "PUT", { listening: "x" }, b.token, 400);

  // Messages: ordered, marked read when opened, limited in size, and private to the pair.
  await request("/messages/" + b.account.id, "POST", { body: "hello  " }, a.token);
  await request("/messages/" + b.account.id, "POST", { body: "again" }, a.token);
  await request("/messages/" + b.account.id, "POST", { body: " " }, a.token, 400);
  await request("/messages/" + b.account.id, "POST", { body: "x".repeat(1001) }, a.token, 400);
  assert.equal((await request("/social", "GET", undefined, b.token)).unread[a.account.id], 2);
  assert.equal((await request("/social", "GET", undefined, b.token)).latest[a.account.id], "again");
  const thread = (await request("/messages/" + a.account.id + "?read=1", "GET", undefined, b.token)).messages;
  assert.deepEqual(thread.map((m) => [m.body, m.mine]), [["hello", false], ["again", false]]);
  assert.equal((await request("/social", "GET", undefined, b.token)).unread[a.account.id], undefined);
  const later = (await request("/messages/" + a.account.id + "?after=" + thread[0].id, "GET", undefined, b.token)).messages;
  assert.deepEqual(later.map((m) => m.body), ["again"]);
  await request("/messages/" + a.account.id, "GET", undefined, c.token, 403);

  // A request from the other side accepts straight away; removing a friend clears the chat.
  await request("/friends/request", "POST", { username: name(c) }, a.token);
  assert.equal((await request("/friends/request", "POST", { username: name(a) }, c.token)).accepted, true);
  await request("/friends/decline", "POST", { id: c.account.id }, a.token);
  await request("/friends/" + b.account.id, "DELETE", undefined, a.token);
  assert.equal((await request("/social", "GET", undefined, a.token)).friends.some((p) => p.id === b.account.id), false);
  await request("/messages/" + b.account.id, "GET", undefined, a.token, 403);
});

test("Blend: a friend's songs, friends only, and a switch to keep them private", async () => {
  const nonce = crypto.randomUUID().replaceAll("-", "").slice(0, 11);
  const password = "local-" + crypto.randomUUID();
  const make = (tag) => request("/register", "POST", { email: tag + nonce + "@example.test", username: tag + "_" + nonce, password });
  const [a, b, c] = [await make("ba"), await make("bb"), await make("bc")];
  const track = (id) => ({ id: "ytmusic:" + id, title: "Song " + id, artist: "Artist", platform: "ytmusic", source: "https://music.youtube.com/watch?v=" + id });
  const tracks = { "ytmusic:aaaaaaaaaaa": track("aaaaaaaaaaa"), "ytmusic:bbbbbbbbbbb": track("bbbbbbbbbbb") };
  await request("/music", "PUT", { revision: 0, snapshot: { tracks, playlists: {}, favorites: ["ytmusic:aaaaaaaaaaa"], history: [{ track_id: "ytmusic:bbbbbbbbbbb", started_at: 1 }] } }, b.token);
  // Not friends yet: nothing to see.
  await request("/friends/" + b.account.id + "/taste", "GET", undefined, a.token, 403);
  await request("/friends/request", "POST", { username: b.account.username }, a.token);
  await request("/friends/accept", "POST", { id: a.account.id }, b.token);
  const blend = await request("/friends/" + b.account.id + "/taste", "GET", undefined, a.token);
  assert.deepEqual(blend.tracks.map((t) => t.id), ["ytmusic:aaaaaaaaaaa", "ytmusic:bbbbbbbbbbb"]);
  // Someone who is not a friend still sees nothing, and the owner can switch sharing off.
  await request("/friends/" + b.account.id + "/taste", "GET", undefined, c.token, 403);
  assert.equal((await request("/me", "PATCH", { share_taste: false }, b.token)).account.share_taste, false);
  await request("/friends/" + b.account.id + "/taste", "GET", undefined, a.token, 403);
});

test("friend suggestions: friends of friends, most shared first, never people you already know", async () => {
  const nonce = crypto.randomUUID().replaceAll("-", "").slice(0, 11);
  const password = "local-" + crypto.randomUUID();
  const make = (tag) => request("/register", "POST", { email: tag + nonce + "@example.test", username: tag + "_" + nonce, password });
  const [a, b, c, d] = [await make("sa"), await make("sb"), await make("sc"), await make("sd")];
  const befriend = async (x, y) => { await request("/friends/request", "POST", { username: y.account.username }, x.token); await request("/friends/accept", "POST", { id: x.account.id }, y.token); };
  assert.deepEqual((await request("/friends/suggestions", "GET", undefined, a.token)).suggestions, []);
  await befriend(a, b); await befriend(b, c); await befriend(a, d); await befriend(d, c);
  const list = (await request("/friends/suggestions", "GET", undefined, a.token)).suggestions;
  assert.deepEqual(list.map((p) => [p.id, p.mutual]), [[c.account.id, 2]]);
  // A profile card shows the friends you share; strangers' cards stay closed.
  const card = await request("/people/" + c.account.id, "GET", undefined, a.token);
  assert.equal(card.relation, "none");
  assert.deepEqual(card.mutual.map((p) => p.id).sort(), [b.account.id, d.account.id].sort());
  assert.equal((await request("/people/" + b.account.id, "GET", undefined, a.token)).relation, "friend");
  const stranger = await make("se");
  await request("/people/" + a.account.id, "GET", undefined, stranger.token, 403);
  // Once a request is out, they stop being suggested.
  await request("/friends/request", "POST", { username: c.account.username }, a.token);
  assert.deepEqual((await request("/friends/suggestions", "GET", undefined, a.token)).suggestions, []);
});

test("dislikes sync with the library and stay out of a Blend", async () => {
  const nonce = crypto.randomUUID().replaceAll("-", "").slice(0, 11);
  const password = "local-" + crypto.randomUUID();
  const make = (tag) => request("/register", "POST", { email: tag + nonce + "@example.test", username: tag + "_" + nonce, password });
  const [a, b] = [await make("da"), await make("db")];
  const track = (id) => ({ id: "ytmusic:" + id, title: "Song " + id, artist: "Artist", platform: "ytmusic", source: "https://music.youtube.com/watch?v=" + id });
  const tracks = { "ytmusic:aaaaaaaaaaa": track("aaaaaaaaaaa"), "ytmusic:bbbbbbbbbbb": track("bbbbbbbbbbb") };
  await request("/music", "PUT", { revision: 0, snapshot: { tracks, playlists: {}, favorites: ["ytmusic:aaaaaaaaaaa"], dislikes: ["ytmusic:bbbbbbbbbbb", "ytmusic:unknown"], history: [{ track_id: "ytmusic:bbbbbbbbbbb", started_at: 1 }] } }, b.token);
  const saved = await request("/music", "GET", undefined, b.token);
  assert.deepEqual(saved.snapshot.dislikes, ["ytmusic:bbbbbbbbbbb"]);
  await request("/friends/request", "POST", { username: b.account.username }, a.token);
  await request("/friends/accept", "POST", { id: a.account.id }, b.token);
  const blend = await request("/friends/" + b.account.id + "/taste", "GET", undefined, a.token);
  assert.deepEqual(blend.tracks.map((t) => t.id), ["ytmusic:aaaaaaaaaaa"]);
});

test("profile cover picture", async () => {
  const nonce = crypto.randomUUID().replaceAll("-", "").slice(0, 11);
  const password = "local-" + crypto.randomUUID();
  const a = await request("/register", "POST", { email: nonce + "@example.test", username: "c_" + nonce, password });
  const b = await request("/register", "POST", { email: "d_" + nonce + "@example.test", username: "d_" + nonce, password });
  const png = Buffer.from("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==", "base64");
  const put = (type, body, token) => fetch(base + "/me/banner", { method: "PUT", headers: { "content-type": type, authorization: "Bearer " + token }, body });
  assert.equal(a.account.banner_url, null);
  assert.equal((await put("image/png", Buffer.from("not an image"), a.token)).status, 400);
  assert.equal((await put("image/png", Buffer.alloc(410_000, 1), a.token)).status, 413);
  const stored = await (await put("image/png", png, a.token)).json();
  assert.match(stored.account.banner_url, /\/banner\/[0-9a-f-]{36}\?v=1$/);
  const image = await fetch(stored.account.banner_url.replace("https://roynit.dangz.org/api/account", base));
  assert.deepEqual(Buffer.from(await image.arrayBuffer()), png);
  // Friends see the cover version, strangers see nothing of the account.
  await request("/friends/request", "POST", { username: a.account.username }, b.token);
  await request("/friends/accept", "POST", { id: b.account.id }, a.token);
  assert.equal((await request("/social", "GET", undefined, b.token)).friends[0].banner_version, 1);
  assert.equal((await request("/me/banner", "DELETE", undefined, a.token)).account.banner_url, null);
  assert.equal((await fetch(stored.account.banner_url.replace("https://roynit.dangz.org/api/account", base))).status, 404);
});

test("username changes and passwords", async () => {
  const nonce = crypto.randomUUID().replaceAll("-", "").slice(0, 11);
  const password = "local-" + crypto.randomUUID();
  const a = await request("/register", "POST", { email: nonce + "@example.test", username: "u_" + nonce, password });
  const b = await request("/register", "POST", { email: "v_" + nonce + "@example.test", username: "v_" + nonce, password });
  assert.equal(a.account.has_password, true);
  assert.equal(a.account.has_google, false);
  assert.equal(a.account.orbs, 0);
  // Six characters is the minimum for a password.
  await request("/register", "POST", { email: "w_" + nonce + "@example.test", username: "w_" + nonce, password: "12345" }, undefined, 400);
  await request("/register", "POST", { email: "w_" + nonce + "@example.test", username: "w_" + nonce, password: "123456" });

  // Usernames are unique: a check says so before saving, and saving a taken one is refused.
  assert.deepEqual(await request("/username/check?name=v_" + nonce, "GET", undefined, a.token), { valid: true, available: false, username: "v_" + nonce });
  assert.equal((await request("/username/check?name=new_" + nonce, "GET", undefined, a.token)).available, true);
  assert.equal((await request("/username/check?name=u_" + nonce, "GET", undefined, a.token)).available, true, "your own name is not taken");
  assert.equal((await request("/username/check?name=a!", "GET", undefined, a.token)).valid, false);
  const taken = await request("/me", "PATCH", { username: "@V_" + nonce }, a.token, 409);
  assert.match(taken.error, /already taken/);
  await request("/me", "PATCH", { username: "no spaces" }, a.token, 400);
  assert.equal((await request("/me", "PATCH", { username: "@New_" + nonce }, a.token)).account.username, "new_" + nonce);
  await request("/login", "POST", { login: "new_" + nonce, password });
  await request("/login", "POST", { login: "u_" + nonce, password }, undefined, 401);

  // Changing a password needs the current one, and signs other devices out.
  const second = await request("/login", "POST", { login: "new_" + nonce, password });
  const next = "another-" + crypto.randomUUID();
  await request("/me/password", "PUT", { password: next }, a.token, 401);
  await request("/me/password", "PUT", { current: "wrong-password-123", password: next }, a.token, 401);
  await request("/me/password", "PUT", { current: password, password: "short" }, a.token, 400);
  await request("/me/password", "PUT", { current: password, password: next }, a.token);
  await request("/login", "POST", { login: "new_" + nonce, password }, undefined, 401);
  await request("/login", "POST", { login: "new_" + nonce, password: next });
  await request("/me", "GET", undefined, a.token);
  await request("/me", "GET", undefined, second.token, 401);
});

test("forgot password: emailed code, limits and a fresh sign-in", async () => {
  const nonce = crypto.randomUUID().replaceAll("-", "").slice(0, 11);
  const password = "local-" + crypto.randomUUID();
  const email = "r_" + nonce + "@example.test";
  const a = await request("/register", "POST", { email, username: "r_" + nonce, password });
  const lastCode = async () => (await request("/test/outbox?to=" + encodeURIComponent(email))).body?.match(/\b(\d{6})\b/)?.[1];

  // Reset codes only go to an address that has been confirmed.
  const confirm = (await request("/test/outbox?to=" + encodeURIComponent(email))).body.match(/\b(\d{6})\b/)[1];
  await request("/email/verify", "POST", { code: confirm }, a.token);

  // The same quiet answer for an account that does not exist, and no mail for it.
  assert.deepEqual(await request("/password/forgot", "POST", { login: "nobody_" + nonce + "@example.test" }), { ok: true });
  assert.deepEqual(await request("/password/forgot", "POST", { login: "R_" + nonce }), { ok: true });
  const code = await lastCode();
  assert.match(code, /^\d{6}$/);

  const next = "fresh-" + crypto.randomUUID();
  const reset = (body, status) => request("/password/reset", "POST", { login: email, password: next, ...body }, undefined, status);
  await reset({ code: code === "000000" ? "111111" : "000000" }, 400);
  await reset({ code: "12ab" }, 400);
  await reset({ code, password: "abc" }, 400);
  const signedIn = await reset({ code });
  assert.equal(signedIn.account.id, a.account.id);
  assert.ok(signedIn.token);
  // The code works once, the old password and old sessions are gone, the new password works.
  await reset({ code }, 400);
  await request("/me", "GET", undefined, a.token, 401);
  await request("/login", "POST", { login: email, password }, undefined, 401);
  await request("/login", "POST", { login: email, password: next });

  // Five wrong tries burn the code, even for the right one afterwards.
  await request("/password/forgot", "POST", { login: email });
  const live = await lastCode();
  const wrong = live === "999999" ? "888888" : "999999";
  for (let i = 0; i < 5; i++) await reset({ code: wrong }, 400);
  await reset({ code: live }, 400);
});

test("email verification at sign-up", async () => {
  const nonce = crypto.randomUUID().replaceAll("-", "").slice(0, 11);
  const password = "local-" + crypto.randomUUID();
  const email = "e_" + nonce + "@example.test";
  const a = await request("/register", "POST", { email, username: "e_" + nonce, password });
  const mailTo = async (to) => (await request("/test/outbox?to=" + encodeURIComponent(to))).body;
  const codeIn = (body) => body.match(/\b(\d{6})\b/)[1];
  assert.equal(a.account.email_verified, false);
  const first = codeIn(await mailTo(email));

  // Until the address is confirmed, "forgot password" sends nothing new.
  assert.deepEqual(await request("/password/forgot", "POST", { login: email }), { ok: true });
  assert.match(await mailTo(email), /verification code/);

  // A wrong code is refused; a typo in the address can be fixed, which mails a fresh code to the new one.
  await request("/email/verify", "POST", { code: first === "000000" ? "111111" : "000000" }, a.token, 400);
  const fixed = "f_" + nonce + "@example.test";
  assert.equal((await request("/me/email", "PUT", { email: fixed }, a.token)).account.email, fixed);
  await request("/me/email", "PUT", { email: "not-an-email" }, a.token, 400);
  await request("/email/verify", "POST", { code: first }, a.token, 400);
  const second = codeIn(await mailTo(fixed));
  const done = await request("/email/verify", "POST", { code: second }, a.token);
  assert.equal(done.account.email_verified, true);
  assert.equal((await request("/me", "GET", undefined, a.token)).account.email_verified, true);
  await request("/me/email", "PUT", { email }, a.token, 400);
  await request("/email/resend", "POST", undefined, a.token, 400);

  // Now a reset code is mailed.
  await request("/password/forgot", "POST", { login: fixed });
  assert.match(await mailTo(fixed), /reset code/);
});

test("daily quests: the server counts the listening, each quest pays once, and fakes earn nothing", async () => {
  const nonce = crypto.randomUUID().replaceAll("-", "").slice(0, 11);
  const password = "local-" + crypto.randomUUID();
  const a = await request("/register", "POST", { email: "qa" + nonce + "@example.test", username: "qa_" + nonce, password });
  const at = (t) => ({ "x-test-now": String(t) });
  const DAY = 1_900_000_000 - (1_900_000_000 % 86400) - 7 * 3600 + 86400 * 3; // 00:00 in Vietnam, a few days out
  const beat = (t, song, position) => request("/presence", "PUT", { listening: { title: "Song " + song, artist: "Artist " + song, duration: 240, position, playing: true } }, a.token, 200, at(t));
  const quests = () => request("/quests", "GET", undefined, a.token, 200, at(DAY + 3600));
  let state = await quests();
  assert.equal(state.quests.length, 3);
  assert.deepEqual(state.quests.map((q) => q.tier), ["easy", "medium", "hard"]);
  assert.ok(state.quests.every((q) => q.progress === 0 && !q.claimed));
  await request("/quests/claim", "POST", { id: state.quests[0].id }, a.token, 409, at(DAY + 3600));
  await request("/quests/claim", "POST", { id: "nonsense" }, a.token, 404, at(DAY + 3600));

  // Heartbeats that do not move the song along earn nothing, and neither does a long silence between two.
  let t = DAY + 3600;
  await beat(t, 1, 10);
  await beat(t + 30, 1, 10);       // 30 s passed but the song did not move
  await beat(t + 400, 1, 440);     // far too long a gap
  state = await quests();
  assert.ok(state.quests.every((q) => q.progress === 0), JSON.stringify(state.quests));

  // Honest listening: new songs, moving positions, 40 s apart.
  t += 1000;
  for (let song = 0; song < 20; song++) for (let k = 0; k < 4; k++) { t += 40; await beat(t, song, 5 + k * 40); }
  state = await quests();
  const easy = state.quests[0];
  assert.ok(easy.done, JSON.stringify(state.quests));
  const before = state.orbs;
  const paid = await request("/quests/claim", "POST", { id: easy.id }, a.token, 200, at(DAY + 3600));
  assert.equal(paid.orbs, before + easy.orbs);
  assert.ok(paid.quests[0].claimed);
  await request("/quests/claim", "POST", { id: easy.id }, a.token, 409, at(DAY + 3600));
  // The next day starts clean.
  const next = await request("/quests", "GET", undefined, a.token, 200, at(DAY + 86400 + 60));
  assert.ok(next.quests.every((q) => q.progress === 0 && !q.claimed));
});

test("shop: buy once, wear what you own, friends see it", async () => {
  const nonce = crypto.randomUUID().replaceAll("-", "").slice(0, 11);
  const password = "local-" + crypto.randomUUID();
  const make = (tag) => request("/register", "POST", { email: tag + nonce + "@example.test", username: tag + "_" + nonce, password });
  const [a, b] = [await make("ha"), await make("hb")];
  await request("/friends/request", "POST", { username: b.account.username }, a.token);
  await request("/friends/accept", "POST", { id: a.account.id }, b.token);
  let shop = await request("/shop", "GET", undefined, a.token);
  assert.equal(shop.orbs, 0);
  assert.ok(shop.items.length >= 20);
  const deco = shop.items.find((i) => i.kind === "decoration");
  await request("/shop/buy", "POST", { item: deco.id, variant: deco.variants[0] }, a.token, 409);      // no Orbs yet
  await request("/shop/buy", "POST", { item: "nope", variant: "x" }, a.token, 404);
  await request("/me/equip", "POST", { kind: "decoration", item: deco.id, variant: deco.variants[0] }, a.token, 403); // not owned
  await request("/shop/test-grant", "POST", { orbs: deco.price * 2 - 1 }, a.token);
  shop = await request("/shop/buy", "POST", { item: deco.id, variant: deco.variants[0] }, a.token);
  assert.equal(shop.orbs, deco.price - 1);
  assert.deepEqual(shop.owned, [{ item: deco.id, variant: deco.variants[0] }]);
  await request("/shop/buy", "POST", { item: deco.id, variant: deco.variants[0] }, a.token, 409);      // already own it
  await request("/shop/buy", "POST", { item: deco.id, variant: deco.variants[1] }, a.token, 409);      // one Orb short
  shop = await request("/me/equip", "POST", { kind: "decoration", item: deco.id, variant: deco.variants[0] }, a.token);
  assert.equal(shop.equipped.decoration, deco.id + ":" + deco.variants[0]);
  await request("/me/equip", "POST", { kind: "frame", item: deco.id, variant: deco.variants[0] }, a.token, 404); // wrong kind
  const seen = await request("/people/" + a.account.id, "GET", undefined, b.token);
  assert.equal(seen.equipped.decoration, deco.id + ":" + deco.variants[0]);
  assert.equal((await request("/me", "GET", undefined, a.token)).account.equipped.decoration, deco.id + ":" + deco.variants[0]);
  shop = await request("/me/equip", "POST", { kind: "decoration", item: null }, a.token);
  assert.deepEqual(shop.equipped, {});
});

test("chat: replies quote the message they answer, and each person can react once per message", async () => {
  const nonce = crypto.randomUUID().replaceAll("-", "").slice(0, 11);
  const password = "local-" + crypto.randomUUID();
  const make = (tag) => request("/register", "POST", { email: tag + nonce + "@example.test", username: tag + "_" + nonce, password });
  const [a, b, c] = [await make("ra"), await make("rb"), await make("rc")];
  await request("/friends/request", "POST", { username: b.account.username }, a.token);
  await request("/friends/accept", "POST", { id: a.account.id }, b.token);
  const sent = await request("/messages/" + b.account.id, "POST", { body: "quoted one" }, a.token);
  const answered = await request("/messages/" + a.account.id, "POST", { body: "answer", reply_to: sent.id }, b.token);
  await request("/messages/" + a.account.id, "POST", { body: "x", reply_to: 999999 }, b.token, 404);
  let view = await request("/messages/" + a.account.id + "?after=" + (answered.id - 1), "GET", undefined, b.token);
  assert.equal(view.messages[0].reply.body, "quoted one");
  assert.equal(view.messages[0].reply.mine, false);
  const heart = "❤️";
  const thumbs = "👍";
  await request("/messages/" + a.account.id + "/" + sent.id + "/react", "POST", { emoji: heart }, b.token);
  await request("/messages/" + a.account.id + "/" + sent.id + "/react", "POST", { emoji: thumbs }, b.token);
  await request("/messages/" + b.account.id + "/" + sent.id + "/react", "POST", { emoji: heart }, a.token);
  view = await request("/messages/" + b.account.id + "?after=" + answered.id, "GET", undefined, a.token);
  assert.deepEqual(view.reactions[sent.id].map((r) => [r.emoji, r.mine]).sort(), [[heart, true], [thumbs, false]].sort());
  await request("/messages/" + a.account.id + "/" + sent.id + "/react", "POST", { emoji: "hello" }, b.token, 400);
  await request("/messages/" + a.account.id + "/" + sent.id + "/react", "POST", { emoji: null }, b.token);
  view = await request("/messages/" + b.account.id + "?after=" + answered.id, "GET", undefined, a.token);
  assert.deepEqual(view.reactions[sent.id].map((r) => r.emoji), [heart]);
  // A stranger can neither read nor react.
  await request("/messages/" + a.account.id + "/" + sent.id + "/react", "POST", { emoji: heart }, c.token, 403);
});

test("inbox wait: answers at once for a new message and times out quietly when there is none", async () => {
  const nonce = crypto.randomUUID().replaceAll("-", "").slice(0, 11);
  const password = "local-" + crypto.randomUUID();
  const make = (tag) => request("/register", "POST", { email: tag + nonce + "@example.test", username: tag + "_" + nonce, password });
  const [a, b] = [await make("wa"), await make("wb")];
  await request("/friends/request", "POST", { username: b.account.username }, a.token);
  await request("/friends/accept", "POST", { id: a.account.id }, b.token);
  const start = await request("/inbox/wait?after=-1", "GET", undefined, b.token);
  assert.equal(start.id, 0);
  const waiting = request("/inbox/wait?after=" + start.id, "GET", undefined, b.token);
  await new Promise((resolve) => setTimeout(resolve, 600));
  const sent = await request("/messages/" + b.account.id, "POST", { body: "hi" }, a.token);
  const woke = await waiting;
  assert.equal(woke.id, sent.id);
  assert.equal((await request("/inbox/wait?after=-1", "GET", undefined, a.token)).id, 0, "your own messages never wake you");
});

test("quick icon: shared by both friends, shown in the friend list and the chat, validated", async () => {
  const nonce = crypto.randomUUID().replaceAll("-", "").slice(0, 11);
  const password = "local-" + crypto.randomUUID();
  const make = (tag) => request("/register", "POST", { email: tag + nonce + "@example.test", username: tag + "_" + nonce, password });
  const [a, b, c] = [await make("qa"), await make("qb"), await make("qc")];
  await request("/friends/request", "POST", { username: b.account.username }, a.token);
  await request("/friends/accept", "POST", { id: a.account.id }, b.token);
  assert.equal((await request("/messages/" + b.account.id, "GET", undefined, a.token)).quick, null);
  assert.equal((await request("/messages/" + b.account.id + "/quick", "PUT", { emoji: "🥰" }, a.token)).quick, "🥰");
  assert.equal((await request("/messages/" + a.account.id, "GET", undefined, b.token)).quick, "🥰", "the other side sees it");
  assert.equal((await request("/social", "GET", undefined, b.token)).friends[0].quick, "🥰");
  await request("/messages/" + b.account.id + "/quick", "PUT", { emoji: "not an emoji" }, a.token, 400);
  await request("/messages/" + a.account.id + "/quick", "PUT", { emoji: "🔥" }, c.token, 403);
  await request("/messages/" + b.account.id + "/quick", "PUT", { emoji: "" }, a.token);
  assert.equal((await request("/messages/" + a.account.id, "GET", undefined, b.token)).quick, null, "empty goes back to the default");
});

test("playlist covers: a picture per playlist that follows the library, validated and removable", async () => {
  const nonce = crypto.randomUUID().replaceAll("-", "").slice(0, 11);
  const a = await request("/register", "POST", { email: "pc" + nonce + "@example.test", username: "pc_" + nonce, password: "local-" + crypto.randomUUID() });
  const png = new Uint8Array([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 1, 2, 3, 4]);
  const put = (id, body, type = "image/png", token = a.token) => fetch(base + "/playlists/" + id + "/cover", { method: "PUT", headers: { authorization: "Bearer " + token, "content-type": type }, body });
  const first = await put("list-aaaa1111", png);
  assert.equal(first.status, 200);
  const v1 = (await first.json()).version;
  assert.ok(v1 > 0);
  const second = await (await put("list-aaaa1111", png)).json();
  assert.ok(second.version > v1, "every change raises the version");
  assert.equal((await put("list-aaaa1111", new Uint8Array([1, 2, 3, 4]), "image/png")).status, 400, "bytes must look like an image");
  assert.equal((await put("list-aaaa1111", png, "image/gif")).status, 400);
  const shown = await fetch(base + "/playlist-cover/" + a.account.id + "/list-aaaa1111");
  assert.equal(shown.status, 200);
  assert.equal(shown.headers.get("content-type"), "image/png");
  assert.equal((await fetch(base + "/playlist-cover/" + a.account.id + "/other-list-1")).status, 404);
  // the library snapshot keeps the version for every device
  const track = { id: "ytmusic:abcdefghijk", title: "t", artist: "a", album: "", duration: 1, source: "https://music.youtube.com/watch?v=abcdefghijk", platform: "ytmusic", cover: null };
  const saved = await request("/music", "PUT", { revision: 0, snapshot: { tracks: { [track.id]: track }, playlists: { "list-aaaa1111": { id: "list-aaaa1111", name: "Mine", tracks: [track.id], cover: second.version } }, favorites: [], history: [] } }, a.token);
  const back = await request("/music", "GET", undefined, a.token);
  assert.equal(back.snapshot.playlists["list-aaaa1111"].cover, second.version);
  const gone = await fetch(base + "/playlists/list-aaaa1111/cover", { method: "DELETE", headers: { authorization: "Bearer " + a.token } });
  assert.ok((await gone.json()).version < 0, "taking it away is a negative version");
  assert.equal((await fetch(base + "/playlist-cover/" + a.account.id + "/list-aaaa1111")).status, 404);
});

test("chat media: photos and voice messages are checked, stored, shown in the chat and fetched by their key", async () => {
  const nonce = crypto.randomUUID().replaceAll("-", "").slice(0, 11);
  const password = "local-" + crypto.randomUUID();
  const make = (tag) => request("/register", "POST", { email: tag + nonce + "@example.test", username: tag + "_" + nonce, password });
  const [a, b, c] = [await make("ma"), await make("mb"), await make("mc")];
  await request("/friends/request", "POST", { username: b.account.username }, a.token);
  await request("/friends/accept", "POST", { id: a.account.id }, b.token);
  const png = new Uint8Array([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 9, 8, 7, 6]);
  const m4a = new Uint8Array([0, 0, 0, 24, 0x66, 0x74, 0x79, 0x70, 0x4d, 0x34, 0x41, 0x20, 1, 2, 3, 4]);
  const send = (to, query, body, type, token = a.token) => fetch(base + "/messages/" + to + "/media" + query, { method: "POST", headers: { authorization: "Bearer " + token, "content-type": type }, body });
  const photo = await send(b.account.id, "?kind=image", png, "image/png");
  assert.equal(photo.status, 200);
  const sent = await photo.json();
  assert.equal(sent.media.kind, "image");
  assert.equal(sent.body, "📷 Photo");
  const file = await fetch(sent.media.url.replace("https://roynit.dangz.org/api/account", base));
  assert.equal(file.status, 200);
  assert.equal(file.headers.get("content-type"), "image/png");
  assert.deepEqual(new Uint8Array(await file.arrayBuffer()), png);
  assert.equal((await send(b.account.id, "?kind=image", new Uint8Array([1, 2, 3, 4]), "image/png")).status, 400, "bytes must look like a photo");
  assert.equal((await send(b.account.id, "?kind=voice", m4a, "audio/mp4")).status, 400, "a voice message needs its length");
  assert.equal((await send(b.account.id, "?kind=voice&seconds=5", png, "audio/mp4")).status, 400, "bytes must look like sound");
  assert.equal((await send(b.account.id, "?kind=video", png, "image/png")).status, 400);
  assert.equal((await send(a.account.id, "?kind=image", png, "image/png", c.token)).status, 403, "only friends");
  assert.equal((await send(b.account.id, "?kind=image", new Uint8Array(500_000).fill(1), "image/png")).status, 413, "too big");
  const voice = await (await send(b.account.id, "?kind=voice&seconds=7", m4a, "audio/mp4")).json();
  assert.equal(voice.media.seconds, 7);
  const chat = await request("/messages/" + a.account.id, "GET", undefined, b.token);
  assert.deepEqual(chat.messages.map((m) => m.media?.kind), ["image", "voice"]);
  assert.equal(chat.messages[0].media.expired, false);
  assert.equal((await fetch(base + "/media/" + "0".repeat(32))).status, 404);
  assert.ok([401, 404].includes((await fetch(base + "/media/not-a-key")).status));
  // taking the friend away takes the files too
  await request("/friends/" + a.account.id, "DELETE", undefined, b.token);
  assert.equal((await fetch(sent.media.url.replace("https://roynit.dangz.org/api/account", base))).status, 404);
});
