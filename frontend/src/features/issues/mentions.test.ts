import { describe, expect, it } from 'vitest';

import {
  addPicked,
  fromWire,
  insertMention,
  matchMentionables,
  mentionName,
  mentionQuery,
  mentionsIn,
  pickedLabel,
  splitMentions,
  toWire,
  type Mentionable,
  type PickedMention,
} from './mentions';

const ADA = 'd6b5915c-4605-7bcb-805f-46f6433df656';
const BOT = '01cbdeb6-76f5-73d7-800d-7403e6bcc2f4';
const ada: Mentionable = { kind: 'user', id: ADA, name: 'Ada Lovelace' };
const adaShort: Mentionable = {
  kind: 'user',
  id: '85f2ef98-7b76-74c3-8c08-1acef84e0a73',
  name: 'Ada',
};
const bot: Mentionable = { kind: 'agent', id: BOT, name: 'Researcher' };
/** The mentions as the composer holds them, each picked in turn. */
const pick = (...ms: Mentionable[]) =>
  ms.reduce<PickedMention[]>((picked, m) => addPicked(picked, m), []);

describe('the tokens a comment carries', () => {
  it('are cut out of the text in order', () => {
    const body = `@[Ada Lovelace](user:${ADA}) asks @[Researcher](agent:${BOT}) to look.`;
    expect(splitMentions(body)).toEqual([
      { kind: 'mention', mention: ada },
      { kind: 'text', text: ' asks ' },
      { kind: 'mention', mention: bot },
      { kind: 'text', text: ' to look.' },
    ]);
    expect(mentionsIn(`${body} Thanks @[Ada](user:${ADA})`)).toEqual([ada, bot]);
  });

  it('read the kind and the id in any case, as the server does', () => {
    expect(mentionsIn(`@[Ada](USER:${ADA.toUpperCase()})`)).toEqual([
      { kind: 'user', id: ADA, name: 'Ada' },
    ]);
  });

  it('leave what only looks like a mention as text', () => {
    for (const body of [
      'mail ada@example.com or @Ada',
      `@[](user:${ADA})`,
      `@[Ada\nLovelace](user:${ADA})`,
      `@[Ada](team:${ADA})`,
      `@[Ada](user:${ADA.replaceAll('-', '')})`,
      `@[Ada](user:${ADA}`,
      `@[${'n'.repeat(101)}](user:${ADA})`,
    ]) {
      expect(splitMentions(body)).toEqual([{ kind: 'text', text: body }]);
    }
    // A name counts characters, not UTF-16 units: a hundred emoji are still one name.
    expect(mentionsIn(`@[${'🙂'.repeat(100)}](user:${ADA})`)).toHaveLength(1);
  });
});

describe('the composer', () => {
  it('sends each picked @Name as its token, the longest name first', () => {
    const text = 'Hi @Ada Lovelace and @Ada, ask @Researcher. Not mail@Ada or @Adam.';
    expect(toWire(text, pick(adaShort, ada, bot))).toBe(
      `Hi @[Ada Lovelace](user:${ADA}) and @[Ada](user:${adaShort.id}), ask ` +
        `@[Researcher](agent:${BOT}). Not mail@Ada or @Adam.`,
    );
    // Nothing picked, nothing changes; a picked name that was deleted again is simply absent.
    expect(toWire('Plain @Ada', [])).toBe('Plain @Ada');
    expect(toWire('Gone', pick(ada))).toBe('Gone');
  });

  it('opens an edit with @Name shown and the mentions it holds', () => {
    const body = `@[Ada Lovelace](user:${ADA}) and @[Researcher](agent:${BOT})`;
    const draft = fromWire(body);
    expect(draft).toEqual({ text: '@Ada Lovelace and @Researcher', picked: pick(ada, bot) });
    expect(toWire(draft.text, draft.picked)).toBe(body);
  });

  it('keeps two mentions of one name apart, written and edited', () => {
    const other: Mentionable = { kind: 'user', id: adaShort.id, name: 'Ada Lovelace' };
    const agent: Mentionable = { kind: 'agent', id: BOT, name: 'Ada Lovelace' };
    // Written: the second Ada Lovelace shows a name of its own, and each @Name sends its token.
    let picked = pick(ada);
    let text = insertMention('@Ad and @Ad', 0, 3, pickedLabel(picked, ada)).text;
    picked = addPicked(picked, other, text);
    text = insertMention(text, 18, 21, pickedLabel(picked, other)).text;
    expect(text).toBe('@Ada Lovelace and @Ada Lovelace (2) ');
    const body = `@[Ada Lovelace](user:${ADA}) and @[Ada Lovelace](user:${other.id}) `;
    expect(toWire(text, picked)).toBe(body);
    // Edited: both come back apart, and saving the text unchanged sends it as it was.
    const twice = `${body}and @[Ada Lovelace](agent:${BOT}) @[Ada Lovelace](user:${ADA})`;
    const draft = fromWire(twice);
    expect(draft.text).toBe(
      '@Ada Lovelace and @Ada Lovelace (2) and @Ada Lovelace (3) @Ada Lovelace',
    );
    expect(draft.picked.map((p) => p.id)).toEqual([ADA, other.id, agent.id]);
    expect(toWire(draft.text, draft.picked)).toBe(twice);
  });

  it('frees the name of a mention taken out of the text', () => {
    const other: Mentionable = { kind: 'user', id: adaShort.id, name: 'Ada Lovelace' };
    const picked = addPicked(pick(ada), other, 'Nobody yet');
    expect(picked).toEqual([{ ...other, label: 'Ada Lovelace' }]);
    // Picked again, a mention keeps the name it shows.
    expect(addPicked(pick(ada, other), ada)).toEqual(pick(ada, other));
    expect(pickedLabel(pick(ada, other), other)).toBe('@Ada Lovelace (2)');
  });

  it('keeps names fit for a token', () => {
    expect(mentionName(' [Ada]\nLovelace ')).toBe('Ada Lovelace');
    expect([...mentionName('x'.repeat(150))]).toHaveLength(100);
    expect(mentionName('[]')).toBe('Someone');
  });

  it('knows when a mention is being typed', () => {
    expect(mentionQuery('@', 1)).toEqual({ start: 0, query: '' });
    expect(mentionQuery('Ask @Ada Lo', 11)).toEqual({ start: 4, query: 'Ada Lo' });
    expect(mentionQuery('Ask @Ada Lo', 7)).toEqual({ start: 4, query: 'Ad' });
    expect(mentionQuery('line\n@re', 8)).toEqual({ start: 5, query: 're' });
    // Not in an email address, not after a space right behind the @, not past 30 characters.
    expect(mentionQuery('ada@ex', 6)).toBeNull();
    expect(mentionQuery('@ x', 3)).toBeNull();
    expect(mentionQuery(`@${'a'.repeat(31)}`, 32)).toBeNull();
  });

  it('puts the picked mention where it was typed', () => {
    expect(insertMention('Ask @Ad please', 4, 7, '@Ada Lovelace')).toEqual({
      text: 'Ask @Ada Lovelace please',
      caret: 18,
    });
    expect(insertMention('Ask @Re', 4, 7, '@Researcher')).toEqual({
      text: 'Ask @Researcher ',
      caret: 16,
    });
  });

  it('lists the candidates that start with what was typed first', () => {
    const people = [
      { kind: 'user' as const, id: '1', name: 'Grace Hopper' },
      { kind: 'user' as const, id: '2', name: 'Ada Lovelace' },
      { kind: 'user' as const, id: '3', name: 'Lovelace Admirer' },
      { kind: 'user' as const, id: '4', name: 'Katherine Johnson' },
    ];
    expect(matchMentionables(people, 'love').map((p) => p.name)).toEqual([
      'Lovelace Admirer',
      'Ada Lovelace',
    ]);
    expect(matchMentionables(people, 'er').map((p) => p.name)).toEqual([
      'Grace Hopper',
      'Katherine Johnson',
      'Lovelace Admirer',
    ]);
    expect(matchMentionables(people, '', 2)).toHaveLength(2);
    expect(matchMentionables(people, 'zz')).toEqual([]);
  });
});
