/**
 * Mentions in comments. A comment's text carries each one as a token, `@[Name](user:<uuid>)` for a
 * member and `@[Name](agent:<uuid>)` for an agent, readable as it is in an export; the composer
 * shows `@Name` and makes the tokens when the comment is sent. The server reads the same tokens
 * (`domain::issue::mentions`) and stays the authority on who hears of them.
 */

/** Whom a mention names. */
export type MentionKind = 'user' | 'agent';

/** A member or an agent a comment can mention. */
export interface Mentionable {
  kind: MentionKind;
  id: string;
  name: string;
}

/** A piece of a comment's text: plain text, or a mention. */
export type MentionSegment =
  { kind: 'text'; text: string } | { kind: 'mention'; mention: Mentionable };

/** Most characters of the name a mention carries (`MENTION_NAME_MAX` on the server). */
export const MENTION_NAME_MAX = 100;
/** Most people and agents one comment may mention (`COMMENT_MENTIONS_MAX` on the server). */
export const COMMENT_MENTIONS_MAX = 20;

const UUID = '[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}';
/** One token; names count code points, as the server counts characters. */
const tokenPattern = () =>
  new RegExp(`@\\[([^\\]\\n]{1,${MENTION_NAME_MAX}})\\]\\((user|agent):(${UUID})\\)`, 'giu');

/** Letters, digits and `_`: what may not touch a mention typed as `@Name`. */
const WORD = '[\\p{L}\\p{N}_]';

/** The key that tells mentions apart: their kind and id. */
export const mentionKey = (m: { kind: MentionKind; id: string }) => `${m.kind}:${m.id}`;

/** A name as a mention carries it: no brackets or line breaks, at most 100 characters. */
export function mentionName(name: string): string {
  const clean = name.replace(/[[\]\r\n]+/g, ' ').trim();
  return [...clean].slice(0, MENTION_NAME_MAX).join('').trim() || 'Someone';
}

/** The token a mention is sent as. */
export const mentionToken = (m: Mentionable) => `@[${mentionName(m.name)}](${m.kind}:${m.id})`;

/** A comment's text cut into plain text and mentions, in order, for showing it. */
export function splitMentions(body: string): MentionSegment[] {
  const out: MentionSegment[] = [];
  let from = 0;
  for (const match of body.matchAll(tokenPattern())) {
    const [whole, name = '', kind = '', id = ''] = match;
    if (match.index > from) out.push({ kind: 'text', text: body.slice(from, match.index) });
    out.push({
      kind: 'mention',
      mention: { kind: kind.toLowerCase() as MentionKind, id: id.toLowerCase(), name },
    });
    from = match.index + whole.length;
  }
  if (from < body.length) out.push({ kind: 'text', text: body.slice(from) });
  return out;
}

/** The members and agents a comment's text mentions, each once, in the order they first appear. */
export function mentionsIn(body: string): Mentionable[] {
  const seen = new Map<string, Mentionable>();
  for (const segment of splitMentions(body)) {
    if (segment.kind === 'mention' && !seen.has(mentionKey(segment.mention)))
      seen.set(mentionKey(segment.mention), segment.mention);
  }
  return [...seen.values()];
}

/** A mention picked into the composer: whom it names, and the `@Name` the text shows for it. */
export interface PickedMention extends Mentionable {
  /** No other picked mention shows it: a second `Alex Kim` is `Alex Kim (2)`. */
  label: string;
}

/** A comment as it is being written: the text shown, and the mentions picked into it. */
export interface MentionDraft {
  text: string;
  picked: PickedMention[];
}

/**
 * `picked` with `m` added, unless it is there already. Each mention gets a label no other picked
 * one has, its name, then `Name (2)`, `Name (3)`…, so two people of one name stay two mentions.
 * Given the text, the mentions it no longer shows are dropped first, freeing their labels.
 */
export function addPicked(picked: PickedMention[], m: Mentionable, text?: string): PickedMention[] {
  const live = text === undefined ? picked : picked.filter((p) => text.includes(`@${p.label}`));
  if (live.some((p) => mentionKey(p) === mentionKey(m))) return live;
  const taken = new Set(live.map((p) => p.label));
  const name = mentionName(m.name);
  let label = name;
  for (let n = 2; taken.has(label); n += 1) label = `${name} (${n})`;
  return [...live, { kind: m.kind, id: m.id, name: m.name, label }];
}

/** The `@Name` the composer shows for `m` among `picked`. */
export function pickedLabel(picked: PickedMention[], m: Mentionable): string {
  const label = picked.find((p) => mentionKey(p) === mentionKey(m))?.label;
  return `@${label ?? mentionName(m.name)}`;
}

const escapeRegExp = (s: string) => s.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');

/**
 * The composer's text as it is sent: each `@Name` of a picked mention becomes its token. Longer
 * labels are tried first, so `@Ada Lovelace` is not taken for `@Ada`. A name that is part of a word
 * or of an email address (`ada@Ada`) stays text, and so does one nobody picked.
 */
export function toWire(text: string, picked: PickedMention[]): string {
  const byLabel = new Map(picked.map((m) => [m.label, m]));
  if (byLabel.size === 0) return text;
  const labels = [...byLabel.keys()].sort((a, b) => b.length - a.length).map(escapeRegExp);
  const pattern = new RegExp(`(^|[^\\p{L}\\p{N}_])@(${labels.join('|')})(?!${WORD})`, 'gu');
  return text.replace(pattern, (_, lead: string, label: string) => {
    const m = byLabel.get(label);
    return m ? `${lead}${mentionToken(m)}` : `${lead}@${label}`;
  });
}

/** A comment's text as the composer shows it, `@Name` for each token, with the mentions it holds. */
export function fromWire(body: string): MentionDraft {
  let picked: PickedMention[] = [];
  const text = splitMentions(body)
    .map((segment) => {
      if (segment.kind === 'text') return segment.text;
      picked = addPicked(picked, segment.mention);
      return pickedLabel(picked, segment.mention);
    })
    .join('');
  return { text, picked };
}

/**
 * The mention being typed just before the caret: where its `@` is and what follows it, or null.
 * The `@` starts the text or follows a space; up to 30 letters, digits, spaces and `._'-` follow.
 */
export function mentionQuery(text: string, caret: number): { start: number; query: string } | null {
  const before = text.slice(0, caret);
  const match = /(^|\s)@((?:[\p{L}\p{N}._'-][\p{L}\p{N}._' -]{0,29})?)$/u.exec(before);
  if (!match) return null;
  return { start: match.index + (match[1]?.length ?? 0), query: match[2] ?? '' };
}

/**
 * The candidates that match what was typed after `@`, at most `limit`: names that start with it
 * first, then names with a word that starts with it, then names that hold it; by name within each.
 */
export function matchMentionables<T extends Mentionable>(
  candidates: T[],
  query: string,
  limit = 8,
): T[] {
  const q = query.trim().toLocaleLowerCase();
  const score = (name: string) => {
    const n = name.toLocaleLowerCase();
    if (!q || n.startsWith(q)) return 0;
    if (n.split(/[\s._-]+/).some((word) => word.startsWith(q))) return 1;
    return n.includes(q) ? 2 : -1;
  };
  return candidates
    .map((c) => ({ c, s: score(c.name) }))
    .filter(({ s }) => s >= 0)
    .sort((a, b) => a.s - b.s || a.c.name.localeCompare(b.c.name))
    .slice(0, limit)
    .map(({ c }) => c);
}

/**
 * The text with the mention being typed (from its `@` at `start` to the caret) replaced by the
 * picked one's `@Name` (`label`) and a space, and where the caret goes after it.
 */
export function insertMention(
  text: string,
  start: number,
  caret: number,
  label: string,
): { text: string; caret: number } {
  const after = text.slice(caret);
  // One space after the name: the one already there, or a new one.
  const insert = /^\s/.test(after) ? label : `${label} `;
  return { text: `${text.slice(0, start)}${insert}${after}`, caret: start + label.length + 1 };
}
