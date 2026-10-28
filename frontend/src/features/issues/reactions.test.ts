import { describe, expect, it } from 'vitest';

import type { Reaction } from '@/schemas/issue';

import { reactionSummary, reactorsLabel, toggleReaction } from './reactions';

const me = { user_id: '0199bbbb-0000-7000-8000-00000000a001', name: 'Ada Lovelace' };
const grace = { user_id: '0199bbbb-0000-7000-8000-00000000a002', name: 'Grace Hopper' };
const alan = { user_id: '0199bbbb-0000-7000-8000-00000000a003', name: 'Alan Turing' };

const thumbs = (users: (typeof me)[], count = users.length): Reaction => ({
  emoji: '👍',
  count,
  reacted: users.includes(me),
  users,
});

describe('toggling a reaction', () => {
  it('adds the reader first among who reacted, or a new emoji after the others', () => {
    expect(toggleReaction([thumbs([grace])], '👍', true, me)).toEqual([thumbs([me, grace])]);
    expect(toggleReaction([thumbs([grace])], '🎉', true, me)).toEqual([
      thumbs([grace]),
      { emoji: '🎉', count: 1, reacted: true, users: [me] },
    ]);
  });

  it('takes the reader out, and the emoji once nobody is left', () => {
    expect(toggleReaction([thumbs([me, grace])], '👍', false, me)).toEqual([thumbs([grace])]);
    expect(toggleReaction([thumbs([me])], '👍', false, me)).toEqual([]);
  });

  it('changes nothing when the reader reacts twice or takes back what is not there', () => {
    const reactions = [thumbs([me, grace])];
    expect(toggleReaction(reactions, '👍', true, me)).toBe(reactions);
    expect(toggleReaction([thumbs([grace])], '👍', false, me)).toEqual([thumbs([grace])]);
    expect(toggleReaction([], '🎉', false, me)).toEqual([]);
  });

  it('keeps naming ten people at most while the count goes on', () => {
    const many = Array.from({ length: 10 }, (_, i) => ({
      user_id: `0199bbbb-0000-7000-8000-0000000b00${String(i).padStart(2, '0')}`,
      name: `P${i}`,
    }));
    const [after] = toggleReaction([thumbs(many, 14)], '👍', true, me);
    expect(after?.count).toBe(15);
    expect(after?.users).toHaveLength(10);
    expect(after?.users[0]).toEqual(me);
  });
});

describe('who reacted', () => {
  it('reads as one name and how many others, the reader as “you” last', () => {
    expect(reactionSummary([], me.user_id).text).toBe('');
    expect(reactionSummary([thumbs([me])], me.user_id).text).toBe('You reacted');
    expect(reactionSummary([thumbs([grace])], me.user_id).text).toBe('Grace Hopper reacted');
    expect(reactionSummary([thumbs([me, grace])], me.user_id).text).toBe(
      'Grace Hopper and you reacted',
    );
    const party: Reaction = { emoji: '🎉', count: 2, reacted: false, users: [alan, grace] };
    const summary = reactionSummary([thumbs([me, grace]), party], me.user_id);
    expect(summary.text).toBe('Grace Hopper and 2 others reacted');
    expect(summary.people).toEqual([grace, alan, me]);
    expect(summary.total).toBe(3);
  });

  it('counts the people past the ten a reaction names', () => {
    expect(reactionSummary([thumbs([grace], 4)], me.user_id).text).toBe(
      'Grace Hopper and 3 others reacted',
    );
  });

  it('does not count twice someone left unnamed under two emoji', () => {
    const ten = Array.from({ length: 10 }, (_, i) => ({
      user_id: `0199bbbb-0000-7000-8000-0000000b00${String(i).padStart(2, '0')}`,
      name: `P${i}`,
    }));
    const party: Reaction = { emoji: '🎉', count: 13, reacted: false, users: ten };
    const summary = reactionSummary([thumbs(ten, 14), party], me.user_id);
    // At least 14 people reacted; maybe the 4 and the 3 past the ten are the same.
    expect(summary.total).toBe(14);
    expect(summary.text).toBe('P0 and 13 others reacted');
  });

  it('names everyone who reacted with an emoji on its chip', () => {
    expect(reactorsLabel(thumbs([me, grace]), me.user_id)).toBe(
      'Grace Hopper and you reacted with 👍',
    );
    expect(reactorsLabel(thumbs([grace, alan], 5), me.user_id)).toBe(
      'Grace Hopper, Alan Turing and 3 others reacted with 👍',
    );
  });
});
