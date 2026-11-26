// Daily listening quests and the Orbs shop. The Worker counts the listening itself, so a client cannot just claim time.
import { SHOP } from "./shop_catalog.mjs";

// Vietnam time (UTC+7): a day starts at 00:00 there.
const DAY_OFFSET = 7 * 3600;
export const dayOf = (seconds) => Math.floor((seconds + DAY_OFFSET) / 86400);
const dayEnd = (day) => (day + 1) * 86400 - DAY_OFFSET;

// Three quests a day, one of each difficulty, each claimable once. The most a person can earn in a day is 15 + 45 + 110.
const POOL = {
  easy: [
    {id: "min10", type: "minutes", target: 10, orbs: 15, title: "Listen for 10 minutes"},
    {id: "songs3", type: "songs", target: 3, orbs: 15, title: "Play 3 different songs"},
  ],
  medium: [
    {id: "min30", type: "minutes", target: 30, orbs: 40, title: "Listen for 30 minutes"},
    {id: "songs8", type: "songs", target: 8, orbs: 40, title: "Play 8 different songs"},
    {id: "artists4", type: "artists", target: 4, orbs: 45, title: "Hear 4 different artists"},
  ],
  hard: [
    {id: "min90", type: "minutes", target: 90, orbs: 100, title: "Listen for 90 minutes"},
    {id: "songs15", type: "songs", target: 15, orbs: 100, title: "Play 15 different songs"},
    {id: "artists8", type: "artists", target: 8, orbs: 110, title: "Hear 8 different artists"},
  ],
};
const SONG_MIN = 20;          // seconds of a song before it counts as played
const BEAT = 45;              // longest gap between two heartbeats that still counts as listening
const DAILY_CAP = 3 * 3600;   // listening beyond three hours a day earns nothing more

export function questsFor(day) {
  const pick = (list, salt) => list[Math.abs(Math.imul(day + salt, 2654435761) >>> 0) % list.length];
  return [{tier: "easy", ...pick(POOL.easy, 1)}, {tier: "medium", ...pick(POOL.medium, 2)}, {tier: "hard", ...pick(POOL.hard, 3)}];
}

const parse = (text, fallback) => { try { const v = JSON.parse(text); return v ?? fallback; } catch { return fallback; } };
const progressOf = (quest, row) => quest.type === "minutes" ? Math.floor((row?.seconds || 0) / 60) : quest.type === "songs" ? parse(row?.songs, []).length : parse(row?.artists, []).length;

/** Credits a heartbeat that says something is playing. Called from the presence route. */
export async function credit(env, userId, listening, at) {
  if (!listening || listening.playing !== true) return;
  const day = dayOf(at);
  const key = (listening.title + "\u0000" + listening.artist).toLowerCase().slice(0, 120);
  let row = await env.DB.prepare("SELECT * FROM quest_day WHERE user_id=? AND day=?").bind(userId, day).first();
  if (!row) {
    await env.DB.prepare("INSERT OR IGNORE INTO quest_day(user_id,day) VALUES (?,?)").bind(userId, day).run();
    row = await env.DB.prepare("SELECT * FROM quest_day WHERE user_id=? AND day=?").bind(userId, day).first();
  }
  const elapsed = at - row.last_at;
  let gained = 0;
  // Only time that follows a heartbeat just before it, and only when the song really moved on by about that much.
  if (row.last_at > 0 && elapsed > 0 && elapsed <= BEAT) {
    const moved = listening.position - row.last_position;
    if (row.last_key !== key || moved >= elapsed * 0.5) gained = Math.min(elapsed, BEAT);
  }
  gained = Math.max(0, Math.min(gained, DAILY_CAP - row.seconds));
  const perSong = parse(row.per_song, {});
  const songs = parse(row.songs, []);
  const artists = parse(row.artists, []);
  if (gained > 0) {
    perSong[key] = (perSong[key] || 0) + gained;
    if (perSong[key] >= SONG_MIN && !songs.includes(key) && songs.length < 60) {
      songs.push(key);
      const artist = listening.artist.toLowerCase().split(",")[0].trim();
      if (artist && !artists.includes(artist) && artists.length < 60) artists.push(artist);
    }
  }
  const keys = Object.keys(perSong);
  if (keys.length > 80) for (const k of keys.slice(0, keys.length - 80)) delete perSong[k];
  await env.DB.prepare("UPDATE quest_day SET seconds=seconds+?,songs=?,artists=?,per_song=?,last_at=?,last_key=?,last_position=? WHERE user_id=? AND day=?")
    .bind(gained, JSON.stringify(songs), JSON.stringify(artists), JSON.stringify(perSong), at, key, listening.position, userId, day).run();
}

export async function questState(env, userId, at) {
  const day = dayOf(at);
  const [row, me] = await Promise.all([
    env.DB.prepare("SELECT * FROM quest_day WHERE user_id=? AND day=?").bind(userId, day).first(),
    env.DB.prepare("SELECT orbs FROM users WHERE id=?").bind(userId).first(),
  ]);
  const claimed = parse(row?.claimed, []);
  return {
    day, resets_in: dayEnd(day) - at, orbs: me?.orbs || 0,
    quests: questsFor(day).map((q) => {
      const progress = Math.min(progressOf(q, row), q.target);
      return {id: q.id, tier: q.tier, title: q.title, target: q.target, progress, orbs: q.orbs, done: progress >= q.target, claimed: claimed.includes(q.id)};
    }),
  };
}

/** Pays out one finished quest. The balance and the claimed flag change together or not at all. */
export async function claim(env, userId, questId, at, HttpError) {
  const day = dayOf(at);
  const quest = questsFor(day).find((q) => q.id === questId);
  if (!quest) throw new HttpError(404, "That quest is not on today's list.");
  const row = await env.DB.prepare("SELECT * FROM quest_day WHERE user_id=? AND day=?").bind(userId, day).first();
  if (!row || progressOf(quest, row) < quest.target) throw new HttpError(409, "Finish the quest first.");
  const old = row.claimed;
  const claimed = parse(old, []);
  if (claimed.includes(quest.id)) throw new HttpError(409, "You already claimed this one today.");
  const next = JSON.stringify([...claimed, quest.id]);
  const [, set] = await env.DB.batch([
    env.DB.prepare("UPDATE users SET orbs=orbs+?1 WHERE id=?2 AND EXISTS(SELECT 1 FROM quest_day WHERE user_id=?2 AND day=?3 AND claimed=?4)").bind(quest.orbs, userId, day, old),
    env.DB.prepare("UPDATE quest_day SET claimed=?1 WHERE user_id=?2 AND day=?3 AND claimed=?4").bind(next, userId, day, old),
    env.DB.prepare("INSERT INTO orb_log(user_id,delta,reason,at) SELECT ?1,?2,?3,?4 WHERE changes()>0").bind(userId, quest.orbs, "quest:" + quest.id, at),
  ]);
  if (set.meta.changes !== 1) throw new HttpError(409, "You already claimed this one today.");
  return questState(env, userId, at);
}

// ---- shop ----

export const equippedOf = (text) => { const v = parse(text, {}); return v && typeof v === "object" && !Array.isArray(v) ? v : {}; };
const KINDS = new Set(["decoration", "nameplate", "frame", "effect"]);

export async function shopState(env, userId) {
  const [me, owned] = await Promise.all([
    env.DB.prepare("SELECT orbs,equipped FROM users WHERE id=?").bind(userId).first(),
    env.DB.prepare("SELECT item,variant FROM inventory WHERE user_id=? ORDER BY acquired_at").bind(userId).all(),
  ]);
  return {orbs: me?.orbs || 0, equipped: equippedOf(me?.equipped), owned: owned.results, items: SHOP.map((i) => ({id: i.id, kind: i.kind, price: i.price, variants: i.variants}))};
}

export async function buy(env, userId, itemId, variant, at, HttpError) {
  const item = SHOP.find((i) => i.id === itemId);
  if (!item || !item.variants.includes(variant)) throw new HttpError(404, "That item is not in the shop.");
  const [spent, , ] = await env.DB.batch([
    env.DB.prepare("UPDATE users SET orbs=orbs-?1 WHERE id=?2 AND orbs>=?1 AND NOT EXISTS(SELECT 1 FROM inventory WHERE user_id=?2 AND item=?3 AND variant=?4)").bind(item.price, userId, item.id, variant),
    env.DB.prepare("INSERT INTO inventory(user_id,item,variant,acquired_at) SELECT ?1,?2,?3,?4 WHERE changes()>0").bind(userId, item.id, variant, at),
    env.DB.prepare("INSERT INTO orb_log(user_id,delta,reason,at) SELECT ?1,?2,?3,?4 WHERE changes()>0").bind(userId, -item.price, "buy:" + item.id + ":" + variant, at),
  ]);
  if (spent.meta.changes !== 1) {
    const have = await env.DB.prepare("SELECT 1 AS x FROM inventory WHERE user_id=? AND item=? AND variant=?").bind(userId, item.id, variant).first();
    throw new HttpError(409, have ? "You already own this." : "Not enough Orbs yet.");
  }
  return shopState(env, userId);
}

export async function equip(env, userId, kind, itemId, variant, HttpError) {
  if (!KINDS.has(kind)) throw new HttpError(400, "Unknown kind.");
  const me = await env.DB.prepare("SELECT equipped FROM users WHERE id=?").bind(userId).first();
  const worn = equippedOf(me?.equipped);
  if (itemId === null || itemId === undefined) delete worn[kind];
  else {
    const item = SHOP.find((i) => i.id === itemId && i.kind === kind);
    if (!item || !item.variants.includes(variant)) throw new HttpError(404, "That item is not in the shop.");
    const own = await env.DB.prepare("SELECT 1 AS x FROM inventory WHERE user_id=? AND item=? AND variant=?").bind(userId, itemId, variant).first();
    if (!own) throw new HttpError(403, "You do not own this yet.");
    worn[kind] = itemId + ":" + variant;
  }
  await env.DB.prepare("UPDATE users SET equipped=? WHERE id=?").bind(JSON.stringify(worn), userId).run();
  return shopState(env, userId);
}
