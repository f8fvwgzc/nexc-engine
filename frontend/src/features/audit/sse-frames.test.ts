import { describe, expect, it } from 'vitest';

import { SseFrameParser } from './sse-frames';

describe('SseFrameParser', () => {
  it('assembles frames across chunk boundaries', () => {
    const parser = new SseFrameParser();
    const raw =
      'event: stage\ndata: {"text":"intent"}\n\n: keep-alive\n\nevent: delta\r\ndata: a\r\ndata: b\r\n\r\ndata: tail\n\n';
    const frames = [];
    for (let i = 0; i < raw.length; i += 5) frames.push(...parser.push(raw.slice(i, i + 5)));
    expect(frames).toEqual([
      { event: 'stage', data: '{"text":"intent"}' },
      { event: 'delta', data: 'a\nb' },
      { event: 'message', data: 'tail' },
    ]);
  });
});
