import type { REACTION_EMOJI, Reaction } from '@/schemas/issue';

/** Someone who reacted. */
export type Reactor = Reaction['users'][number];

/** What each emoji of the allow-list stands for, for a screen reader and a tooltip. */
const EMOJI_NAME: Record<(typeof REACTION_EMOJI)[number], string> = {
  '👍': 'thumbs up',
  '👎': 'thumbs down',
  '😄': 'smile',
  '🎉': 'party',
  '😕': 'confused',
  '❤️': 'heart',
  '🚀': 'rocket',
  '👀': 'eyes',
  '🔥': 'fire',
  '✅': 'done',
};

export const emojiName = (emoji: string): string =>
  EMOJI_NAME[emoji as keyof typeof EMOJI_NAME] ?? emoji;

/**
 * The reactions once the caller reacted with `emoji` (`on`) or took it back: the count moves by
 * one, the caller goes first among who reacted (or is taken out), and an emoji nobody reacts
 * with any more goes. Reacting twice, or taking back what was not there, changes nothing.
 */
export function toggleReaction(
  reactions: Reaction[],
  emoji: string,
  on: boolean,
  me: Reactor,
): Reaction[] {
  const found = reactions.find((r) => r.emoji === emoji);
  const others = (r: Reaction) => r.users.filter((u) => u.user_id !== me.user_id);
  if (on) {
    if (found?.reacted) return reactions;
    if (!found) return [...reactions, { emoji, count: 1, reacted: true, users: [me] }];
    return reactions.map((r) =>
      r === found
        ? { ...r, count: r.count + 1, reacted: true, users: [me, ...others(r)].slice(0, 10) }
        : r,
    );
  }
  if (!found?.reacted) return reactions;
  if (found.count <= 1) return reactions.filter((r) => r !== found);
  return reactions.map((r) =>
    r === found ? { ...r, count: r.count - 1, reacted: false, users: others(r) } : r,
  );
}

/** "Dana", "Dana and you", "Dana, Grace and 3 others": people named first, the caller last. */
function namesOf(people: Reactor[], total: number, me: string | undefined): string {
  const names = [
    ...people.filter((p) => p.user_id !== me).map((p) => p.name),
    ...(people.some((p) => p.user_id === me) ? ['you'] : []),
  ];
  const unnamed = total - names.length;
  if (names.length === 0) return `${total} ${total === 1 ? 'person' : 'people'}`;
  if (unnamed > 0)
    return `${names.join(', ')} and ${unnamed} ${unnamed === 1 ? 'other' : 'others'}`;
  if (names.length <= 1) return names.join('');
  return `${names.slice(0, -1).join(', ')} and ${names.at(-1)}`;
}

const capitalized = (text: string) => text.charAt(0).toUpperCase() + text.slice(1);

/** Who reacted with one emoji, as its chip's tooltip says it. */
export function reactorsLabel(reaction: Reaction, me: string | undefined): string {
  return `${capitalized(namesOf(reaction.users, reaction.count, me))} reacted with ${reaction.emoji}`;
}

/**
 * Who reacted to an entry, for its summary line: each person once (the caller last), how many
 * reacted in all, and the line itself ("Dana and 3 others reacted"). The server names at most ten
 * people per emoji; those past that are counted as others.
 */
export function reactionSummary(
  reactions: Reaction[],
  me: string | undefined,
): { people: Reactor[]; total: number; text: string } {
  const seen = new Map<string, Reactor>();
  let unnamed = 0;
  for (const r of reactions) {
    for (const u of r.users) if (!seen.has(u.user_id)) seen.set(u.user_id, u);
    unnamed += Math.max(0, r.count - r.users.length);
  }
  const people = [...seen.values()].sort(
    (a, b) => Number(a.user_id === me) - Number(b.user_id === me),
  );
  const total = people.length + unnamed;
  if (total === 0) return { people, total, text: '' };
  // One name, then everyone else as a number: "Dana and you" only when that is all of them.
  const first = people[0];
  const text =
    total === 2 && people.length === 2
      ? namesOf(people, total, me)
      : namesOf(first ? [first] : [], total, me);
  return { people, total, text: `${capitalized(text)} reacted` };
}
