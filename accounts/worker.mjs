import { scrypt, timingSafeEqual } from "node:crypto";
import { Buffer } from "node:buffer";
import { connect } from "cloudflare:sockets";

const encoder = new TextEncoder();
const DAY = 86_400;
const PREFIX = "/api/account";
const now = () => Math.floor(Date.now() / 1000);
const hex = (bytes) => Array.from(new Uint8Array(bytes), (b) => b.toString(16).padStart(2, "0")).join("");
const random = () => hex(crypto.getRandomValues(new Uint8Array(32)));
const digest = async (text) => hex(await crypto.subtle.digest("SHA-256", encoder.encode(text)));
const publicUser = ({id, email, username, display_name, google_subject, password, orbs, share_taste, email_verified, bio, avatar_set, avatar_version, banner_set, banner_version, created_at}, base) => ({
  id, email, username, display_name, provider: google_subject ? "google" : "password",
  has_password: !!password, has_google: !!google_subject, orbs: orbs || 0, share_taste: share_taste !== 0, email_verified: !!email_verified,
  bio: bio || "", created_at,
  // The version in the URL changes with every upload, so browsers and the app can cache it forever.
  avatar_url: avatar_set ? base + "/avatar/" + id + "?v=" + avatar_version : null,
  banner_url: banner_set ? base + "/banner/" + id + "?v=" + banner_version : null,
});
const reply = (body, status = 200) => Response.json(body, {
  status, headers: { "cache-control": "no-store", "x-content-type-options": "nosniff" },
});
class HttpError extends Error {
  constructor(status, message) { super(message); this.status = status; }
}
async function json(request, limit = 8192) {
  if (!request.headers.get("content-type")?.startsWith("application/json") || !request.body) {
    throw new HttpError(400, "Expected JSON");
  }
  const reader = request.body.getReader();
  const decoder = new TextDecoder("utf-8", {fatal:true});
  let size = 0;
  let text = "";
  try {
    while (true) {
      const {done, value} = await reader.read();
      if (done) break;
      size += value.byteLength;
      if (size > limit) { await reader.cancel(); throw new HttpError(413, "Request is too large"); }
      text += decoder.decode(value, {stream:true});
    }
    const value = JSON.parse(text + decoder.decode());
    if (!value || typeof value !== "object" || Array.isArray(value)) throw Error();
    return value;
  } catch (error) {
    if (error instanceof HttpError) throw error;
    throw new HttpError(400, "Invalid JSON");
  }
}
async function bytes(request, limit) {
  if (!request.body) throw new HttpError(400, "Expected an image");
  const reader = request.body.getReader();
  const chunks = [];
  let size = 0;
  while (true) {
    const {done, value} = await reader.read();
    if (done) break;
    size += value.byteLength;
    if (size > limit) { await reader.cancel(); throw new HttpError(413, "That image is too large."); }
    chunks.push(value);
  }
  const out = new Uint8Array(size);
  let at = 0;
  for (const chunk of chunks) { out.set(chunk, at); at += chunk.byteLength; }
  return out;
}
// Trust the bytes, not the header: a file must start like the image type it claims to be.
function imageMatches(type, data) {
  const start = (...values) => values.every((value, i) => data[i] === value);
  if (type === "image/jpeg") return start(0xff, 0xd8, 0xff);
  if (type === "image/png") return start(0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a);
  return type === "image/webp" && start(0x52, 0x49, 0x46, 0x46) && data[8] === 0x57 && data[9] === 0x45 && data[10] === 0x42 && data[11] === 0x50;
}
async function avatar(id, env, table = "avatars") {
  if (!/^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/.test(id)) throw new HttpError(404, "Not found.");
  const row = await env.DB.prepare("SELECT content_type,data FROM " + table + " WHERE user_id=?").bind(id).first();
  if (!row) throw new HttpError(404, "Not found.");
  return new Response(new Uint8Array(row.data), { headers: {
    "content-type": row.content_type, "cache-control": "public, max-age=31536000, immutable",
    "x-content-type-options": "nosniff", "content-security-policy": "default-src 'none'",
    "cross-origin-resource-policy": "cross-origin",
  }});
}
function passwordHash(password, salt) {
  return new Promise((resolve, reject) => {
    scrypt(password, salt, 32, { N: 32768, r: 8, p: 3, maxmem: 64 * 1024 * 1024 }, (error, key) => {
      if (error) reject(error);
      else resolve(hex(key));
    });
  });
}
function equal(a, b) {
  const left = Buffer.from(a, "hex");
  const right = Buffer.from(b, "hex");
  return left.length === right.length && timingSafeEqual(left, right);
}
const str = (value) => typeof value === "string" ? value : "";
const emailPattern = /^[^\s@]+@[^\s@]+\.[^\s@]+$/u;
const tokenPattern = /^[0-9a-f]{64}$/;
const usernameOf = (value) => { const name = str(value).trim().replace(/^@/, "").toLowerCase(); return /^[a-z0-9_]{3,24}$/.test(name) ? name : ""; };
async function rate(env, request, scope, max, identifier = "") {
  const stamp = now();
  const ip = request.headers.get("cf-connecting-ip") || "local";
  const bucket = await digest(scope + ":" + (identifier || ip));
  const row = await env.DB.prepare(
    "INSERT INTO auth_limits VALUES (?,1,?) ON CONFLICT(bucket) DO UPDATE SET count=CASE WHEN expires_at<=? THEN 1 ELSE count+1 END,expires_at=CASE WHEN expires_at<=? THEN excluded.expires_at ELSE expires_at END RETURNING count"
  ).bind(bucket, stamp + 600, stamp, stamp).first();
  if (row.count > max) throw new HttpError(429, "Too many attempts. Try again later.");
}
async function session(env, user) {
  const token = random();
  await env.DB.prepare("INSERT INTO sessions VALUES (?,?,?)")
    .bind(await digest(token), user.id, now() + 30 * DAY).run();
  return reply({ token, account: publicUser(user, env.PUBLIC_BASE) });
}
async function authenticate(request, env) {
  const token = request.headers.get("authorization")?.match(/^Bearer ([0-9a-f]{64})$/)?.[1];
  if (!token) throw new HttpError(401,"Sign in to continue.");
  const hash = await digest(token);
  const user = await env.DB.prepare("SELECT users.* FROM sessions JOIN users ON users.id=sessions.user_id WHERE token_hash=? AND expires_at>?")
    .bind(hash, now()).first();
  if (!user) throw new HttpError(401,"Session expired. Sign in again.");
  return {user, hash};
}
async function credentials(request, env, register) {
  await rate(env, request, register ? "register" : "login", register ? Number(env.REGISTER_LIMIT) || 8 : 25);
  const body = await json(request);
  const password = str(body.password);
  if (password.length > 128 || !password) throw new HttpError(400,"Invalid password.");
  if (register) {
    const email = str(body.email).trim().toLowerCase();
    const username = str(body.username).trim().toLowerCase();
    if (email.length > 254 || !emailPattern.test(email) || !/^[a-z0-9_]{3,24}$/.test(username) || password.length < 6) {
      throw new HttpError(400,"Use a valid email, a 3–24 character username and at least 6 password characters.");
    }
    const salt = random();
    const hash = await passwordHash(password, salt);
    const user = await env.DB.prepare(
      "INSERT OR IGNORE INTO users(id,email,username,display_name,password,created_at,email_verified) VALUES (?,?,?,?,?,?,?) RETURNING *"
    ).bind(crypto.randomUUID(), email, username, username, "scrypt:"+salt+":"+hash, now(), mailReady(env) ? 0 : 1).first();
    if (!user) throw new HttpError(409,"That email or username is already in use.");
    if (!user.email_verified) {
      try { await issueEmailCode(env, user); } catch (error) { console.error("Verification email failed", error?.code || error?.name); }
    }
    return session(env, user);
  }
  const login = str(body.login).trim().toLowerCase();
  if (!login || login.length > 254) throw new HttpError(400, "Enter your email or username.");
  await rate(env, request, "identity", 12, login);
  const user = await env.DB.prepare("SELECT * FROM users WHERE password IS NOT NULL AND (email=? OR username=?)")
    .bind(login,login).first();
  const [scheme, salt, expected] = user?.password.split(":") || ["scrypt", "0".repeat(64), "0".repeat(64)];
  const actual = await passwordHash(password, salt);
  if (!user || scheme !== "scrypt" || !equal(actual, expected)) throw new HttpError(401,"Email, username or password is incorrect.");
  return session(env,user);
}
export default {
  async fetch(request, env, ctx) {
    try {
      return await route(request, env, ctx);
    } catch (error) {
      if (error instanceof HttpError) return reply({error:error.message}, error.status);
      console.error("Roynit account request failed", error.name);
      return reply({error:"Account service unavailable."}, 500);
    }
  },
};
async function googleBegin(request, env) {
  if (!env.GOOGLE_CLIENT_ID || !env.GOOGLE_CLIENT_SECRET) throw new HttpError(503,"Google sign-in is not configured.");
  await rate(env,request,"google",12);
  const state = random();
  const pickup = random();
  const verifier = random();
  const challenge = btoa(String.fromCharCode(...new Uint8Array(await crypto.subtle.digest("SHA-256",encoder.encode(verifier)))))
    .replaceAll("+","-").replaceAll("/","_").replace(/=+$/,"");
  const url = new URL("https://accounts.google.com/o/oauth2/v2/auth");
  url.search = new URLSearchParams({
    client_id:env.GOOGLE_CLIENT_ID, redirect_uri:env.PUBLIC_BASE+"/google/callback",
    response_type:"code", scope:"openid email profile", state,
    code_challenge:challenge, code_challenge_method:"S256", prompt:"select_account",
  });
  await env.DB.prepare("INSERT INTO google_attempts(state_hash,pickup_hash,verifier,expires_at) VALUES (?,?,?,?)")
    .bind(await digest(state),await digest(pickup),verifier,now()+600).run();
  return reply({url:url.href,pickup});
}
function oauthPage(success, linked = false) {
  const heading = linked ? (success ? "Account linked" : "Linking was not completed")
    : success ? "You’re signed in" : "Sign-in was not completed";
  return new Response('<!doctype html><html lang="en"><meta charset="utf-8"><meta name="viewport" content="width=device-width"><title>Roynit ID</title><style>body{background:#101014;color:#f9f8ff;font:16px system-ui;display:grid;place-items:center;min-height:90vh}main{padding:3rem;border:1px solid #313039;border-radius:24px;background:#1b1922}small{color:#beabed}</style><main><small>ROYNIT ID</small><h1>'+heading+'</h1><p>You can close this tab and return to the app.</p></main></html>',{
    status:success ? 200 : 400,
    headers:{"content-type":"text/html; charset=utf-8","cache-control":"no-store",
      "referrer-policy":"no-referrer","content-security-policy":"default-src 'none'; style-src 'unsafe-inline'; frame-ancestors 'none'"},
  });
}
async function googleCallback(url, env) {
  const state = url.searchParams.get("state") || "";
  if (!tokenPattern.test(state)) return oauthPage(false);
  const hash = await digest(state);
  const attempt = await env.DB.prepare("UPDATE google_attempts SET status='exchanging' WHERE state_hash=? AND status='pending' AND expires_at>? RETURNING verifier")
    .bind(hash,now()).first();
  if (!attempt) return oauthPage(false);
  try {
    const code = url.searchParams.get("code");
    if (!code) return oauthPage(false);
    const response = await fetch("https://oauth2.googleapis.com/token", {
      method:"POST", body:new URLSearchParams({
        client_id:env.GOOGLE_CLIENT_ID,client_secret:env.GOOGLE_CLIENT_SECRET,
        code,code_verifier:attempt.verifier,grant_type:"authorization_code",redirect_uri:env.PUBLIC_BASE+"/google/callback",
      }),
    });
    if (!response.ok) return oauthPage(false);
    const {access_token} = await response.json();
    if (!access_token) return oauthPage(false);
    const identity = await fetch("https://openidconnect.googleapis.com/v1/userinfo",{
      headers:{authorization:"Bearer "+access_token},
    });
    if (!identity.ok) return oauthPage(false);
    const info = await identity.json();
    if (info.email_verified !== true || !str(info.sub) || !emailPattern.test(str(info.email))) return oauthPage(false);
    const stem = info.email.split("@")[0].toLowerCase().replace(/[^a-z0-9_]/g,"").slice(0,13) || "listener";
    const username = stem+"_"+random().slice(0,8);
    const id = crypto.randomUUID();
    const user = await env.DB.prepare(
      "INSERT INTO users(id,email,username,display_name,google_subject,created_at) VALUES (?,?,?,?,?,?) ON CONFLICT(google_subject) DO UPDATE SET email=excluded.email RETURNING id"
    ).bind(id,info.email,username,str(info.name).trim().slice(0,48)||username,info.sub,now()).first();
    await env.DB.prepare("UPDATE google_attempts SET status='ready',user_id=?,verifier='' WHERE state_hash=?")
      .bind(user.id,hash).run();
    return oauthPage(true);
  } finally {
    await env.DB.prepare("UPDATE google_attempts SET status='failed',verifier='' WHERE state_hash=? AND status='exchanging'")
      .bind(hash).run();
  }
}
async function googlePickup(request, env) {
  const {pickup} = await json(request);
  if (!tokenPattern.test(str(pickup))) throw new HttpError(400,"Invalid sign-in request.");
  const hash = await digest(pickup);
  const record = await env.DB.prepare("DELETE FROM google_attempts WHERE pickup_hash=? AND expires_at>? AND status='ready' RETURNING user_id")
    .bind(hash,now()).first();
  if (record) {
    const user = await env.DB.prepare("SELECT * FROM users WHERE id=?").bind(record.user_id).first();
    return session(env,user);
  }
  const attempt = await env.DB.prepare("SELECT status FROM google_attempts WHERE pickup_hash=? AND expires_at>?")
    .bind(hash,now()).first();
  if (!attempt || attempt.status === "failed") throw new HttpError(410,"Google sign-in expired or was cancelled.");
  return reply({pending:true},202);
}
function musicSnapshot(input) {
  if (!input || typeof input !== "object" || Array.isArray(input)
    || !input.tracks || typeof input.tracks !== "object" || Array.isArray(input.tracks)
    || !input.playlists || typeof input.playlists !== "object" || Array.isArray(input.playlists)
    || !Array.isArray(input.favorites) || !Array.isArray(input.history)) {
    throw new HttpError(400,"Invalid music library.");
  }
  if (Object.keys(input.tracks).length>3000 || Object.keys(input.playlists).length>250 || input.history.length>3000) {
    throw new HttpError(413,"Music library exceeds the sync limit.");
  }
  const tracks = Object.create(null);
  for (const [id, t] of Object.entries(input.tracks)) {
    if (!t || id !== t.id || !["youtube","ytmusic","soundcloud","spotify","deezer","apple","yandex","tidal","vk","tiktok"].includes(t.platform)
       || (typeof t.source !== "string" || !/^https:\/\//i.test(t.source))) continue;
    tracks[id] = {id,title:str(t.title).slice(0,512),artist:str(t.artist).slice(0,512),album:str(t.album).slice(0,512),
      duration:Number.isFinite(t.duration) && t.duration>=0 ? t.duration : 0,source:t.source.slice(0,2048),
      platform:t.platform,cover:/^https:\/\//i.test(str(t.cover)) ? t.cover.slice(0,2048) : null};
  }
  const playlists = Object.create(null);
  for (const [id,p] of Object.entries(input.playlists)) {
    if (p && p.id===id && Array.isArray(p.tracks)) {
      playlists[id]={id,name:str(p.name).slice(0,200),tracks:p.tracks.filter((id)=>typeof id==="string" && tracks[id])};
    }
  }
  return {tracks,playlists,favorites:input.favorites.filter((id)=>typeof id==="string" && tracks[id]),
    dislikes:(Array.isArray(input.dislikes) ? input.dislikes : []).filter((id)=>typeof id==="string" && tracks[id]).slice(0,2000),
    history:input.history.filter((event)=>event && typeof event.track_id==="string" && tracks[event.track_id] && Number.isSafeInteger(event.started_at))
      .map(({track_id,started_at})=>({track_id,started_at}))};
}
// ---- Linked provider accounts: the Worker keeps the refresh token, the app only ever receives short-lived access tokens ----
const PROVIDERS = {
  google: {
    auth:"https://accounts.google.com/o/oauth2/v2/auth", token:"https://oauth2.googleapis.com/token",
    identity:"https://openidconnect.googleapis.com/v1/userinfo", id:"GOOGLE_CLIENT_ID", secret:"GOOGLE_CLIENT_SECRET",
    scope:"openid email profile https://www.googleapis.com/auth/youtube.readonly",
    extra:{access_type:"offline", prompt:"consent", include_granted_scopes:"true"},
    label:(info)=>str(info.email),
  },
  spotify: {
    auth:"https://accounts.spotify.com/authorize", token:"https://accounts.spotify.com/api/token",
    identity:"https://api.spotify.com/v1/me", id:"SPOTIFY_CLIENT_ID", secret:"SPOTIFY_CLIENT_SECRET",
    scope:"playlist-read-private playlist-read-collaborative user-library-read",
    extra:{show_dialog:"true"},
    label:(info)=>str(info.display_name) || str(info.id),
  },
};
// OAUTH_TEST_BASE is only ever set by the local smoke test, which points the providers at a mock server.
const providerUrl = (env, name, key) => env.OAUTH_TEST_BASE ? env.OAUTH_TEST_BASE+"/"+name+"/"+key : PROVIDERS[name][key];
const configured = (env, name) => !!(env.LINK_KEY && env[PROVIDERS[name].id] && env[PROVIDERS[name].secret]);
const available = (env) => ({google:configured(env,"google"),spotify:configured(env,"spotify")});
async function linkKey(env) {
  const raw = env.LINK_KEY ? Buffer.from(env.LINK_KEY,"base64") : null;
  if (!raw || raw.length!==32) throw new HttpError(503,"Account linking is not configured.");
  return crypto.subtle.importKey("raw",raw,"AES-GCM",false,["encrypt","decrypt"]);
}
async function seal(env, text, aad) {
  const iv = crypto.getRandomValues(new Uint8Array(12));
  const sealed = await crypto.subtle.encrypt({name:"AES-GCM",iv,additionalData:encoder.encode(aad)},await linkKey(env),encoder.encode(text));
  return Buffer.concat([iv,Buffer.from(sealed)]).toString("base64");
}
async function unseal(env, value, aad) {
  const raw = Buffer.from(value,"base64");
  const plain = await crypto.subtle.decrypt({name:"AES-GCM",iv:raw.subarray(0,12),additionalData:encoder.encode(aad)},await linkKey(env),raw.subarray(12));
  return new TextDecoder().decode(plain);
}
async function linkList(env, user) {
  const {results} = await env.DB.prepare("SELECT provider,label,linked_at FROM links WHERE user_id=?").bind(user.id).all();
  return {links:results,available:available(env)};
}
async function linkStart(request, env, user, provider) {
  if (!configured(env,provider)) throw new HttpError(503,"This account cannot be linked yet. It has not been set up on the server.");
  await rate(env,request,"link",10,user.id);
  const config = PROVIDERS[provider];
  const state = random();
  const verifier = random();
  const challenge = btoa(String.fromCharCode(...new Uint8Array(await crypto.subtle.digest("SHA-256",encoder.encode(verifier)))))
    .replaceAll("+","-").replaceAll("/","_").replace(/=+$/,"");
  const url = new URL(providerUrl(env,provider,"auth"));
  url.search = new URLSearchParams({
    client_id:env[config.id], redirect_uri:env.PUBLIC_BASE+"/link/callback", response_type:"code", scope:config.scope,
    state, code_challenge:challenge, code_challenge_method:"S256", ...config.extra,
  });
  await env.DB.prepare("INSERT INTO link_attempts(state_hash,user_id,provider,verifier,expires_at) VALUES (?,?,?,?,?)")
    .bind(await digest(state),user.id,provider,verifier,now()+600).run();
  return reply({url:url.href});
}
async function providerToken(env, provider, fields) {
  const config = PROVIDERS[provider];
  try {
    return await fetch(providerUrl(env,provider,"token"),{method:"POST",body:new URLSearchParams({
      client_id:env[config.id], client_secret:env[config.secret], ...fields,
    })});
  } catch {
    throw new HttpError(502,"The provider is not responding. Try again soon.");
  }
}
async function linkCallback(url, env) {
  const state = url.searchParams.get("state") || "";
  if (!tokenPattern.test(state)) return oauthPage(false,true);
  const hash = await digest(state);
  const attempt = await env.DB.prepare("UPDATE link_attempts SET status='exchanging' WHERE state_hash=? AND status='pending' AND expires_at>? RETURNING user_id,provider,verifier")
    .bind(hash,now()).first();
  if (!attempt) return oauthPage(false,true);
  try {
    const code = url.searchParams.get("code");
    if (!code) return oauthPage(false,true);
    const response = await providerToken(env,attempt.provider,{
      grant_type:"authorization_code", code, code_verifier:attempt.verifier, redirect_uri:env.PUBLIC_BASE+"/link/callback",
    });
    if (!response.ok) return oauthPage(false,true);
    const {access_token, refresh_token} = await response.json();
    // Without a refresh token the app would lose access after an hour, so treat it as a failed link.
    if (!access_token || !refresh_token) return oauthPage(false,true);
    let label = "";
    let subject = "";
    try {
      const identity = await fetch(providerUrl(env,attempt.provider,"identity"),{headers:{authorization:"Bearer "+access_token}});
      if (identity.ok) {
        const info = await identity.json();
        label = PROVIDERS[attempt.provider].label(info).slice(0,120);
        subject = str(info.sub);
      }
    } catch {}
    // Linking Google also lets that Google account sign in to this Roynit ID, unless it already belongs to another one.
    if (attempt.provider === "google" && subject) {
      await env.DB.prepare("UPDATE users SET google_subject=?1 WHERE id=?2 AND google_subject IS NULL AND NOT EXISTS (SELECT 1 FROM users WHERE google_subject=?1)")
        .bind(subject,attempt.user_id).run();
    }
    await env.DB.prepare("INSERT INTO links(user_id,provider,label,refresh,linked_at) VALUES (?,?,?,?,?) ON CONFLICT(user_id,provider) DO UPDATE SET label=excluded.label,refresh=excluded.refresh,linked_at=excluded.linked_at")
      .bind(attempt.user_id,attempt.provider,label,await seal(env,refresh_token,attempt.user_id+":"+attempt.provider),now()).run();
    return oauthPage(true,true);
  } finally {
    await env.DB.prepare("DELETE FROM link_attempts WHERE state_hash=?").bind(hash).run();
  }
}
async function linkToken(request, env, user, provider) {
  await rate(env,request,"linktoken",60,user.id);
  const row = await env.DB.prepare("SELECT refresh FROM links WHERE user_id=? AND provider=?").bind(user.id,provider).first();
  if (!row) throw new HttpError(404,"This account is not linked.");
  const aad = user.id+":"+provider;
  const response = await providerToken(env,provider,{grant_type:"refresh_token",refresh_token:await unseal(env,row.refresh,aad)});
  if (!response.ok) {
    // The user revoked access (or the grant expired): drop the link so the app offers to connect again.
    if (response.status===400 || response.status===401) {
      await env.DB.prepare("DELETE FROM links WHERE user_id=? AND provider=?").bind(user.id,provider).run();
      throw new HttpError(409,"Access was revoked. Link the account again.");
    }
    throw new HttpError(502,"The provider is not responding. Try again soon.");
  }
  const {access_token, expires_in, refresh_token} = await response.json();
  if (!access_token) throw new HttpError(502,"The provider is not responding. Try again soon.");
  if (refresh_token) {
    await env.DB.prepare("UPDATE links SET refresh=? WHERE user_id=? AND provider=?").bind(await seal(env,refresh_token,aad),user.id,provider).run();
  }
  return reply({access_token,expires_in:Number.isFinite(expires_in) ? expires_in : 3600});
}
// ---- Forgot password: a 6-digit code emailed to the account, valid for 15 minutes ----
const RESET_SECONDS = 900;
const RESET_TRIES = 5;
// Mail can leave through Gmail SMTP (a Gmail address plus an app password), the Cloudflare Email binding, or,
// in local tests only, a table. Without any of them nothing is sent and signing up skips email verification.
const mailReady = (env) => !!(env.MAIL_TEST_OUTBOX || env.EMAIL || (env.GMAIL_USER && env.GMAIL_APP_PASSWORD));
async function smtpSend(env, to, subject, text) {
  const user = str(env.GMAIL_USER);
  const socket = connect({ hostname: env.SMTP_HOST || "smtp.gmail.com", port: Number(env.SMTP_PORT) || 465 },
    { secureTransport: env.SMTP_PLAIN ? "off" : "on" });
  const reader = socket.readable.getReader();
  const writer = socket.writable.getWriter();
  const decoder = new TextDecoder();
  let pending = "";
  // One complete server reply (it may span several lines; the last one has a space after the code).
  const read = async () => {
    for (;;) {
      const lines = pending.split("\r\n");
      const last = lines.slice(0, -1).findIndex((line) => /^\d{3}( |$)/.test(line));
      if (last >= 0) {
        pending = lines.slice(last + 1).join("\r\n");
        return Number(lines[last].slice(0, 3));
      }
      const { value, done } = await reader.read();
      if (done) throw new Error("smtp closed");
      pending += decoder.decode(value, { stream: true });
    }
  };
  const say = (line) => writer.write(encoder.encode(line + "\r\n"));
  const expect = async (code) => { if (await read() !== code) throw new Error("smtp refused"); };
  const run = async () => {
    await expect(220);
    await say("EHLO roynit.dangz.org"); await expect(250);
    await say("AUTH PLAIN " + btoa("\0" + user + "\0" + str(env.GMAIL_APP_PASSWORD))); await expect(235);
    await say("MAIL FROM:<" + user + ">"); await expect(250);
    await say("RCPT TO:<" + to + ">"); await expect(250);
    await say("DATA"); await expect(354);
    const body = btoa(String.fromCharCode(...encoder.encode(text))).replace(/.{76}/g, "$&\r\n");
    await say(["From: Roy Music <" + user + ">", "To: <" + to + ">", "Subject: " + subject, "Date: " + new Date().toUTCString(),
      "MIME-Version: 1.0", "Content-Type: text/plain; charset=utf-8", "Content-Transfer-Encoding: base64", "", body, "."].join("\r\n"));
    await expect(250);
    await say("QUIT");
  };
  let timer;
  try {
    await Promise.race([run(), new Promise((_, reject) => { timer = setTimeout(() => reject(new Error("smtp timeout")), 15000); })]);
  } finally {
    clearTimeout(timer);
    try { socket.close(); } catch {}
  }
}
async function sendMail(env, to, subject, text) {
  if (!emailPattern.test(to)) throw new HttpError(400, "That email address is not valid.");
  if (env.MAIL_TEST_OUTBOX) {
    await env.DB.prepare("INSERT INTO mail_outbox(to_address,subject,body,created_at) VALUES (?,?,?,?)").bind(to, subject, text, now()).run();
  } else if (env.GMAIL_USER && env.GMAIL_APP_PASSWORD) {
    await smtpSend(env, to, subject, text);
  } else if (env.EMAIL) {
    await env.EMAIL.send({ to, from: { email: env.MAIL_FROM || "no-reply@dangz.org", name: "Roy Music" }, subject, text });
  } else {
    throw new HttpError(503, "Email is not set up on this server.");
  }
}
const sixDigits = () => String(crypto.getRandomValues(new Uint32Array(1))[0] % 1_000_000).padStart(6, "0");
// A code for confirming the address on an account; only its hash is kept.
async function issueEmailCode(env, user) {
  const code = sixDigits();
  await env.DB.prepare("INSERT INTO email_codes(user_id,email,code_hash,expires_at,attempts) VALUES (?,?,?,?,0) ON CONFLICT(user_id) DO UPDATE SET email=excluded.email,code_hash=excluded.code_hash,expires_at=excluded.expires_at,attempts=0")
    .bind(user.id, user.email, await digest(user.id + ":email:" + code), now() + RESET_SECONDS).run();
  await sendMail(env, user.email, "Verify your Roy Music email",
    "Your Roy Music verification code is " + code + ".\n\nIt works for 15 minutes. If you did not create a Roy Music account, ignore this email.");
}
async function accountFor(env, login) {
  // An account that signs in with a password comes first when an email is shared with a Google account.
  return env.DB.prepare("SELECT * FROM users WHERE email=?1 OR username=?1 ORDER BY password IS NULL LIMIT 1").bind(login).first();
}
async function passwordForgot(request, env) {
  await rate(env, request, "forgot", Number(env.FORGOT_LIMIT) || 10);
  const login = str((await json(request)).login).trim().toLowerCase();
  if (!login || login.length > 254) throw new HttpError(400, "Enter your email or username.");
  await rate(env, request, "forgot-identity", 3, login);
  const user = await accountFor(env, login);
  if (user?.email_verified) {
    const code = sixDigits();
    await env.DB.prepare("INSERT INTO password_resets(user_id,code_hash,expires_at,attempts) VALUES (?,?,?,0) ON CONFLICT(user_id) DO UPDATE SET code_hash=excluded.code_hash,expires_at=excluded.expires_at,attempts=0")
      .bind(user.id, await digest(user.id + ":" + code), now() + RESET_SECONDS).run();
    try {
      await sendMail(env, user.email, "Your Roy Music reset code",
        "Your Roy Music password reset code is " + code + ".\n\nIt works for 15 minutes. If you did not ask for it, ignore this email: your password has not changed.");
    } catch (error) {
      console.error("Reset email failed", error?.code || error?.name);
    }
  }
  // The same answer whether or not the account exists, so this cannot be used to find accounts.
  return reply({ ok: true });
}
async function passwordReset(request, env) {
  await rate(env, request, "reset", Number(env.FORGOT_LIMIT) || 20);
  const body = await json(request);
  const login = str(body.login).trim().toLowerCase();
  const code = str(body.code).trim();
  const password = str(body.password);
  if (!login || login.length > 254) throw new HttpError(400, "Enter your email or username.");
  if (password.length < 6 || password.length > 128) throw new HttpError(400, "Use a password of 6 to 128 characters.");
  await rate(env, request, "reset-identity", 10, login);
  const wrong = new HttpError(400, "That code is not right or has expired.");
  const user = /^\d{6}$/.test(code) ? await accountFor(env, login) : null;
  if (!user) throw wrong;
  const row = await env.DB.prepare("SELECT * FROM password_resets WHERE user_id=?").bind(user.id).first();
  if (!row || row.expires_at <= now()) throw wrong;
  if (row.code_hash !== await digest(user.id + ":" + code)) {
    const tries = await env.DB.prepare("UPDATE password_resets SET attempts=attempts+1 WHERE user_id=? RETURNING attempts").bind(user.id).first();
    if (tries.attempts >= RESET_TRIES) await env.DB.prepare("DELETE FROM password_resets WHERE user_id=?").bind(user.id).run();
    throw wrong;
  }
  const salt = random();
  let updated;
  try {
    updated = await env.DB.prepare("UPDATE users SET password=? WHERE id=? RETURNING *")
      .bind("scrypt:" + salt + ":" + await passwordHash(password, salt), user.id).first();
  } catch (error) {
    if (String(error?.message).includes("UNIQUE")) throw new HttpError(409, "Another account already signs in with this email and a password.");
    throw error;
  }
  await env.DB.batch([
    env.DB.prepare("DELETE FROM password_resets WHERE user_id=?").bind(user.id),
    env.DB.prepare("DELETE FROM sessions WHERE user_id=?").bind(user.id),
  ]);
  return session(env, updated);
}
// ---- Friends, presence and direct messages ----
const uuidPattern = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/;
const ONLINE_FOR = 75;
const pairOf = (a, b) => a < b ? [a, b] : [b, a];
const person = (row) => ({id:row.id, username:row.username, display_name:row.display_name, bio:row.bio || "",
  avatar_version:row.avatar_set ? row.avatar_version : 0, banner_version:row.banner_set ? row.banner_version : 0});
async function friendship(env, me, other) {
  const [low, high] = pairOf(me, other);
  return env.DB.prepare("SELECT * FROM friendships WHERE low=? AND high=?").bind(low, high).first();
}
async function requireFriend(env, me, other) {
  if (!uuidPattern.test(other)) throw new HttpError(404, "Not found.");
  const row = await friendship(env, me, other);
  if (row?.status !== "accepted") throw new HttpError(403, "You are not friends with that person.");
}
async function socialSnapshot(env, user) {
  const stamp = now();
  const {results} = await env.DB.prepare(
    "SELECT f.status,f.requester,u.id,u.username,u.display_name,u.bio,u.avatar_set,u.avatar_version,u.banner_set,u.banner_version,p.seen_at,p.invisible,p.listening "
    + "FROM friendships f JOIN users u ON u.id=CASE WHEN f.low=?1 THEN f.high ELSE f.low END "
    + "LEFT JOIN presence p ON p.user_id=u.id WHERE f.low=?1 OR f.high=?1 ORDER BY f.created_at DESC"
  ).bind(user.id).all();
  const unread = Object.create(null);
  const counts = await env.DB.prepare("SELECT sender,COUNT(*) AS n FROM messages WHERE (low=?1 OR high=?1) AND sender!=?1 AND read=0 GROUP BY sender").bind(user.id).all();
  for (const row of counts.results) unread[row.sender] = row.n;
  const out = {now:stamp, friends:[], incoming:[], outgoing:[], unread};
  for (const row of results) {
    if (row.status === "accepted") {
      const online = !row.invisible && row.seen_at > stamp - ONLINE_FOR;
      let listening = null;
      if (online && row.listening) { try { listening = JSON.parse(row.listening); } catch {} }
      out.friends.push({...person(row), online, listening, last_seen:row.invisible ? null : row.seen_at || null});
    } else if (row.requester === user.id) out.outgoing.push(person(row));
    else out.incoming.push(person(row));
  }
  return out;
}
function listeningState(input) {
  if (input === null || input === undefined) return null;
  if (typeof input !== "object" || Array.isArray(input)) throw new HttpError(400, "Invalid listening state.");
  const number = (value, max) => Number.isFinite(value) && value >= 0 ? Math.min(value, max) : 0;
  const title = str(input.title).trim().slice(0, 200);
  if (!title) return null;
  return {title, artist:str(input.artist).trim().slice(0, 200),
    cover:/^https:\/\//i.test(str(input.cover)) ? input.cover.slice(0, 2048) : null,
    duration:number(input.duration, 86_400), position:number(input.position, 86_400), playing:input.playing === true, at:now()};
}
async function socialRoute(request, env, user, path, url) {
  if (path === "/social" && request.method === "GET") return reply(await socialSnapshot(env, user));
  if (path === "/presence" && request.method === "PUT") {
    await rate(env, request, "presence", 400, user.id);
    const body = await json(request, 4096);
    const listening = listeningState(body.listening);
    await env.DB.prepare("INSERT INTO presence(user_id,invisible,seen_at,listening) VALUES (?,?,?,?) ON CONFLICT(user_id) DO UPDATE SET invisible=excluded.invisible,seen_at=excluded.seen_at,listening=excluded.listening")
      .bind(user.id, body.invisible === true ? 1 : 0, now(), listening && JSON.stringify(listening)).run();
    return reply({ok:true});
  }
  if (path === "/friends/request" && request.method === "POST") {
    await rate(env, request, "friend", 20, user.id);
    const username = str((await json(request)).username).trim().replace(/^@/, "").toLowerCase();
    if (!/^[a-z0-9_]{3,24}$/.test(username)) throw new HttpError(400, "Enter a username, 3–24 letters, numbers or underscores.");
    const other = await env.DB.prepare("SELECT id,display_name FROM users WHERE username=?").bind(username).first();
    if (!other) throw new HttpError(404, "Nobody has that username.");
    if (other.id === user.id) throw new HttpError(400, "That is your own username.");
    const existing = await friendship(env, user.id, other.id);
    if (existing?.status === "accepted") throw new HttpError(409, "You are already friends.");
    if (existing && existing.requester === user.id) throw new HttpError(409, "Friend request already sent.");
    const [low, high] = pairOf(user.id, other.id);
    if (existing) {
      await env.DB.prepare("UPDATE friendships SET status='accepted' WHERE low=? AND high=?").bind(low, high).run();
      return reply({ok:true, accepted:true, name:other.display_name});
    }
    await env.DB.prepare("INSERT INTO friendships(low,high,requester,status,created_at) VALUES (?,?,?,'pending',?)").bind(low, high, user.id, now()).run();
    return reply({ok:true, accepted:false, name:other.display_name});
  }
  const friendAction = path.match(/^\/friends\/(accept|decline)$/);
  if (friendAction && request.method === "POST") {
    const id = str((await json(request)).id);
    if (!uuidPattern.test(id)) throw new HttpError(404, "Not found.");
    const [low, high] = pairOf(user.id, id);
    if (friendAction[1] === "accept") {
      const row = await env.DB.prepare("UPDATE friendships SET status='accepted' WHERE low=? AND high=? AND status='pending' AND requester!=? RETURNING low").bind(low, high, user.id).first();
      if (!row) throw new HttpError(404, "That request is gone.");
    } else {
      await env.DB.prepare("DELETE FROM friendships WHERE low=? AND high=? AND status='pending'").bind(low, high).run();
    }
    return reply({ok:true});
  }
  const removal = path.match(/^\/friends\/([0-9a-f-]{36})$/);
  if (removal && request.method === "DELETE") {
    if (!uuidPattern.test(removal[1])) throw new HttpError(404, "Not found.");
    const [low, high] = pairOf(user.id, removal[1]);
    await env.DB.batch([
      env.DB.prepare("DELETE FROM friendships WHERE low=? AND high=?").bind(low, high),
      env.DB.prepare("DELETE FROM messages WHERE low=? AND high=?").bind(low, high),
    ]);
    return reply({ok:true});
  }
  if (path === "/friends/suggestions" && request.method === "GET") {
    // People you may know: friends of your friends you have no request with yet, most shared friends first.
    await rate(env, request, "suggest", 60, user.id);
    const {results} = await env.DB.prepare(
      "WITH mine AS (SELECT CASE WHEN low=?1 THEN high ELSE low END AS id FROM friendships WHERE (low=?1 OR high=?1) AND status='accepted'), "
      + "related AS (SELECT CASE WHEN low=?1 THEN high ELSE low END AS id FROM friendships WHERE low=?1 OR high=?1) "
      + "SELECT u.id,u.username,u.display_name,u.bio,u.avatar_set,u.avatar_version,u.banner_set,u.banner_version,COUNT(*) AS mutual "
      + "FROM friendships f JOIN users u ON u.id=CASE WHEN f.low IN (SELECT id FROM mine) THEN f.high ELSE f.low END "
      + "WHERE f.status='accepted' AND (f.low IN (SELECT id FROM mine) OR f.high IN (SELECT id FROM mine)) "
      + "AND u.id!=?1 AND u.id NOT IN (SELECT id FROM related) GROUP BY u.id ORDER BY mutual DESC,u.display_name LIMIT 12"
    ).bind(user.id).all();
    return reply({suggestions:results.map((row) => ({...person(row), mutual:row.mutual}))});
  }
  const taste = path.match(/^\/friends\/([0-9a-f-]{36})\/taste$/);
  if (taste && request.method === "GET") {
    // Blend: a friend's most-loved songs, so the app can mix them with yours. Friends only, and only when they allow it.
    await rate(env, request, "taste", 60, user.id);
    await requireFriend(env, user.id, taste[1]);
    const row = await env.DB.prepare("SELECT u.share_taste,u.display_name,m.snapshot FROM users u LEFT JOIN music m ON m.user_id=u.id WHERE u.id=?").bind(taste[1]).first();
    if (!row || row.share_taste === 0) throw new HttpError(403, "They keep their taste private.");
    const snapshot = row.snapshot ? JSON.parse(row.snapshot) : {tracks:{}, favorites:[], history:[]};
    const score = new Map();
    for (const id of snapshot.favorites || []) score.set(id, (score.get(id) || 0) + 5);
    for (const id of snapshot.dislikes || []) score.delete(id);
    for (const event of (snapshot.history || []).slice(-400)) score.set(event.track_id, (score.get(event.track_id) || 0) + 1);
    for (const id of snapshot.dislikes || []) score.delete(id);
    const ranked = [...score.entries()].sort((x, y) => y[1] - x[1]).map(([id]) => snapshot.tracks[id]).filter(Boolean).slice(0, 60);
    return reply({name: row.display_name, tracks: ranked});
  }
  const chat = path.match(/^\/messages\/([0-9a-f-]{36})$/);
  if (chat) {
    await requireFriend(env, user.id, chat[1]);
    const [low, high] = pairOf(user.id, chat[1]);
    if (request.method === "GET") {
      const after = Number.parseInt(url.searchParams.get("after") || "0", 10) || 0;
      const {results} = await env.DB.prepare("SELECT id,sender,body,created_at FROM messages WHERE low=? AND high=? AND id>? ORDER BY id DESC LIMIT 100").bind(low, high, after).all();
      if (url.searchParams.get("read") === "1") {
        await env.DB.prepare("UPDATE messages SET read=1 WHERE low=? AND high=? AND sender=? AND read=0").bind(low, high, chat[1]).run();
      }
      return reply({messages:results.reverse().map((m) => ({id:m.id, mine:m.sender === user.id, body:m.body, at:m.created_at}))});
    }
    if (request.method === "POST") {
      await rate(env, request, "message", 150, user.id);
      const body = str((await json(request, 4096)).body).replace(/\r\n/g, "\n").trim();
      if (!body || body.length > 1000) throw new HttpError(400, "Write a message up to 1000 characters.");
      const row = await env.DB.prepare("INSERT INTO messages(low,high,sender,body,created_at) VALUES (?,?,?,?,?) RETURNING id,created_at").bind(low, high, user.id, body, now()).first();
      return reply({id:row.id, mine:true, body, at:row.created_at});
    }
  }
  return null;
}
async function route(request, env, ctx) {
  const url = new URL(request.url);
  if (!url.pathname.startsWith(PREFIX+"/")) throw new HttpError(404,"Not found.");
  const path = url.pathname.slice(PREFIX.length);
  if (path === "/health" && request.method === "GET") return reply({ready:true,google:!!(env.GOOGLE_CLIENT_ID && env.GOOGLE_CLIENT_SECRET),link:available(env),email:mailReady(env)});
  if (request.method === "POST" && (path === "/register" || path === "/login")) {
    ctx.waitUntil(env.DB.batch([
      env.DB.prepare("DELETE FROM auth_limits WHERE expires_at<=?").bind(now()),
      env.DB.prepare("DELETE FROM google_attempts WHERE expires_at<=?").bind(now()),
      env.DB.prepare("DELETE FROM sessions WHERE expires_at<=?").bind(now()),
      env.DB.prepare("DELETE FROM link_attempts WHERE expires_at<=?").bind(now()),
    ]).catch(()=>{}));
    return credentials(request,env,path==="/register");
  }
  if (request.method==="POST" && path==="/password/forgot") return passwordForgot(request,env);
  if (request.method==="POST" && path==="/password/reset") return passwordReset(request,env);
  if (env.MAIL_TEST_OUTBOX && request.method==="GET" && path==="/test/outbox") {
    const row = await env.DB.prepare("SELECT body FROM mail_outbox WHERE to_address=? ORDER BY id DESC LIMIT 1").bind(url.searchParams.get("to") || "").first();
    return reply({ body: row?.body || null });
  }
  if (request.method==="POST" && path==="/google/start") return googleBegin(request,env);
  if (request.method==="POST" && path==="/google/pickup") return googlePickup(request,env);
  if (request.method==="GET" && path==="/google/callback") return googleCallback(url,env);
  if (request.method==="GET" && path.startsWith("/avatar/")) return avatar(path.slice(8),env);
  if (request.method==="GET" && path.startsWith("/banner/")) return avatar(path.slice(8),env,"banners");
  if (request.method==="GET" && path==="/link/callback") return linkCallback(url,env);
  const {user,hash} = await authenticate(request,env);
  const social = await socialRoute(request,env,user,path,url);
  if (social) return social;
  if (path==="/links" && request.method==="GET") return reply(await linkList(env,user));
  const linkRoute = path.match(/^\/link\/(google|spotify)(?:\/(start|token))?$/);
  if (linkRoute) {
    const [, provider, action] = linkRoute;
    if (action==="start" && request.method==="POST") return linkStart(request,env,user,provider);
    if (action==="token" && request.method==="POST") return linkToken(request,env,user,provider);
    if (!action && request.method==="DELETE") {
      await env.DB.prepare("DELETE FROM links WHERE user_id=? AND provider=?").bind(user.id,provider).run();
      return reply(await linkList(env,user));
    }
  }
  if (path==="/me" && request.method==="GET") return reply({account:publicUser(user,env.PUBLIC_BASE)});
  if (path==="/me" && request.method==="PATCH") {
    const body = await json(request);
    const sets = [];
    const values = [];
    if ("display_name" in body) {
      const name = str(body.display_name).trim();
      if (!name || name.length>48 || /[\u0000-\u001f]/.test(name)) throw new HttpError(400,"Enter a display name up to 48 characters.");
      sets.push("display_name=?"); values.push(name);
    }
    if ("share_taste" in body) {
      sets.push("share_taste=?"); values.push(body.share_taste === true ? 1 : 0);
    }
    if ("bio" in body) {
      const bio = str(body.bio).replace(/\s+/g," ").trim();
      if (bio.length>160) throw new HttpError(400,"Keep your bio to 160 characters or fewer.");
      sets.push("bio=?"); values.push(bio);
    }
    if ("username" in body) {
      await rate(env, request, "username", 10, user.id);
      const name = usernameOf(body.username);
      if (!name) throw new HttpError(400,"Use 3–24 letters, numbers or underscores.");
      sets.push("username=?"); values.push(name);
    }
    if (!sets.length) throw new HttpError(400,"Nothing to update.");
    let updated;
    try {
      updated = await env.DB.prepare("UPDATE users SET "+sets.join(",")+" WHERE id=? RETURNING *").bind(...values,user.id).first();
    } catch (error) {
      if (String(error?.message).includes("UNIQUE")) throw new HttpError(409,"That username is already taken. Try another.");
      throw error;
    }
    return reply({account:publicUser(updated,env.PUBLIC_BASE)});
  }
  if (path==="/username/check" && request.method==="GET") {
    await rate(env, request, "usercheck", 200, user.id);
    const name = usernameOf(url.searchParams.get("name"));
    if (!name) return reply({valid:false, available:false});
    const taken = await env.DB.prepare("SELECT id FROM users WHERE username=? AND id!=?").bind(name,user.id).first();
    return reply({valid:true, available:!taken, username:name});
  }
  if (path==="/me/password" && request.method==="PUT") {
    await rate(env, request, "password", 10, user.id);
    const body = await json(request);
    const next = str(body.password);
    if (next.length < 6 || next.length > 128) throw new HttpError(400,"Use a password of 6 to 128 characters.");
    if (user.password) {
      const [scheme, salt, expected] = user.password.split(":");
      if (scheme !== "scrypt" || !equal(await passwordHash(str(body.current), salt), expected)) throw new HttpError(401,"Your current password is not correct.");
    }
    const salt = random();
    try {
      await env.DB.prepare("UPDATE users SET password=? WHERE id=?").bind("scrypt:"+salt+":"+await passwordHash(next, salt), user.id).run();
    } catch (error) {
      if (String(error?.message).includes("UNIQUE")) throw new HttpError(409,"Another account already signs in with this email and a password.");
      throw error;
    }
    // Changing a password signs every other device out.
    await env.DB.prepare("DELETE FROM sessions WHERE user_id=? AND token_hash!=?").bind(user.id, hash).run();
    return reply({account:publicUser({...user, password:"set"},env.PUBLIC_BASE)});
  }
  if (path==="/email/verify" && request.method==="POST") {
    if (user.email_verified) return reply({account:publicUser(user,env.PUBLIC_BASE)});
    await rate(env, request, "emailverify", 20, user.id);
    const code = str((await json(request)).code).trim();
    const wrong = new HttpError(400,"That code is not right or has expired.");
    const row = await env.DB.prepare("SELECT * FROM email_codes WHERE user_id=?").bind(user.id).first();
    if (!/^\d{6}$/.test(code) || !row || row.expires_at <= now() || row.email !== user.email) throw wrong;
    if (row.code_hash !== await digest(user.id + ":email:" + code)) {
      const tries = await env.DB.prepare("UPDATE email_codes SET attempts=attempts+1 WHERE user_id=? RETURNING attempts").bind(user.id).first();
      if (tries.attempts >= RESET_TRIES) await env.DB.prepare("DELETE FROM email_codes WHERE user_id=?").bind(user.id).run();
      throw wrong;
    }
    const [, done] = await env.DB.batch([
      env.DB.prepare("DELETE FROM email_codes WHERE user_id=?").bind(user.id),
      env.DB.prepare("UPDATE users SET email_verified=1 WHERE id=? RETURNING *").bind(user.id),
    ]);
    return reply({account:publicUser(done.results[0],env.PUBLIC_BASE)});
  }
  if (path==="/email/resend" && request.method==="POST") {
    if (user.email_verified) throw new HttpError(400,"Your email is already verified.");
    await rate(env, request, "emailresend", 3, user.id);
    try { await issueEmailCode(env, user); } catch (error) {
      console.error("Verification email failed", error?.code || error?.name);
      throw new HttpError(503,"Could not send the email right now. Try again in a moment.");
    }
    return reply({ok:true});
  }
  if (path==="/me/email" && request.method==="PUT") {
    if (user.email_verified) throw new HttpError(400,"Your email is already verified.");
    await rate(env, request, "emailchange", 5, user.id);
    const email = str((await json(request)).email).trim().toLowerCase();
    if (email.length > 254 || !emailPattern.test(email)) throw new HttpError(400,"Enter a valid email address.");
    let updated;
    try {
      updated = await env.DB.prepare("UPDATE users SET email=? WHERE id=? RETURNING *").bind(email, user.id).first();
    } catch (error) {
      if (String(error?.message).includes("UNIQUE")) throw new HttpError(409,"That email is already used by another account.");
      throw error;
    }
    try { await issueEmailCode(env, updated); } catch (error) {
      console.error("Verification email failed", error?.code || error?.name);
      throw new HttpError(503,"Could not send the email right now. Try again in a moment.");
    }
    return reply({account:publicUser(updated,env.PUBLIC_BASE)});
  }
  if (path==="/me/avatar" && request.method==="PUT") {
    await rate(env, request, "avatar", 20, user.id);
    const type = (request.headers.get("content-type") || "").split(";")[0].trim().toLowerCase();
    const data = await bytes(request, 150_000);
    if (!data.length || !imageMatches(type, data)) throw new HttpError(400,"Use a JPEG, PNG or WebP image under 150 KB.");
    const [, update] = await env.DB.batch([
      env.DB.prepare("INSERT INTO avatars(user_id,content_type,data) VALUES (?,?,?) ON CONFLICT(user_id) DO UPDATE SET content_type=excluded.content_type,data=excluded.data")
        .bind(user.id,type,data.buffer),
      env.DB.prepare("UPDATE users SET avatar_set=1,avatar_version=avatar_version+1 WHERE id=? RETURNING *").bind(user.id),
    ]);
    return reply({account:publicUser(update.results[0],env.PUBLIC_BASE)});
  }
  if (path==="/me/avatar" && request.method==="DELETE") {
    const [, update] = await env.DB.batch([
      env.DB.prepare("DELETE FROM avatars WHERE user_id=?").bind(user.id),
      env.DB.prepare("UPDATE users SET avatar_set=0,avatar_version=avatar_version+1 WHERE id=? RETURNING *").bind(user.id),
    ]);
    return reply({account:publicUser(update.results[0],env.PUBLIC_BASE)});
  }
  if (path==="/me/banner" && request.method==="PUT") {
    await rate(env, request, "banner", 20, user.id);
    const type = (request.headers.get("content-type") || "").split(";")[0].trim().toLowerCase();
    const data = await bytes(request, 400_000);
    if (!data.length || !imageMatches(type, data)) throw new HttpError(400,"Use a JPEG, PNG or WebP image under 400 KB.");
    const [, update] = await env.DB.batch([
      env.DB.prepare("INSERT INTO banners(user_id,content_type,data) VALUES (?,?,?) ON CONFLICT(user_id) DO UPDATE SET content_type=excluded.content_type,data=excluded.data")
        .bind(user.id,type,data.buffer),
      env.DB.prepare("UPDATE users SET banner_set=1,banner_version=banner_version+1 WHERE id=? RETURNING *").bind(user.id),
    ]);
    return reply({account:publicUser(update.results[0],env.PUBLIC_BASE)});
  }
  if (path==="/me/banner" && request.method==="DELETE") {
    const [, update] = await env.DB.batch([
      env.DB.prepare("DELETE FROM banners WHERE user_id=?").bind(user.id),
      env.DB.prepare("UPDATE users SET banner_set=0,banner_version=banner_version+1 WHERE id=? RETURNING *").bind(user.id),
    ]);
    return reply({account:publicUser(update.results[0],env.PUBLIC_BASE)});
  }
  if (path==="/logout" && request.method==="POST") {
    await env.DB.prepare("DELETE FROM sessions WHERE token_hash=?").bind(hash).run();
    return reply({ok:true});
  }
  if (path==="/music" && request.method==="GET") {
    const row = await env.DB.prepare("SELECT * FROM music WHERE user_id=?").bind(user.id).first();
    return reply(row ? {revision:row.revision,updated_at:row.updated_at,snapshot:JSON.parse(row.snapshot)} : {revision:0});
  }
  if (path==="/music" && request.method==="PUT") {
    const body = await json(request,950_000);
    if (!Number.isSafeInteger(body.revision) || body.revision<0) throw new HttpError(400,"Invalid library revision.");
    const snapshot = JSON.stringify(musicSnapshot(body.snapshot));
    const stamp = now();
    const write = body.revision === 0
      ? env.DB.prepare("INSERT OR IGNORE INTO music VALUES (?,1,?,?) RETURNING revision").bind(user.id,snapshot,stamp)
      : env.DB.prepare("UPDATE music SET revision=revision+1,snapshot=?,updated_at=? WHERE user_id=? AND revision=? RETURNING revision").bind(snapshot,stamp,user.id,body.revision);
    const row = await write.first();
    if (!row) throw new HttpError(409,"The cloud library changed. Download it before uploading again.");
    return reply({revision:row.revision,updated_at:stamp});
  }
  throw new HttpError(404,"Not found.");
}
