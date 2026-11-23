import assert from "node:assert/strict";
import { createServer } from "node:http";
import test from "node:test";

const base = "http://127.0.0.1:8790/api/account";
async function request(path, method = "GET", body, token, expected = 200) {
  const headers = {};
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
      if (req.url === "/google/identity") return send(200, { email: "listener@example.test", sub: "google-sub-1" });
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
