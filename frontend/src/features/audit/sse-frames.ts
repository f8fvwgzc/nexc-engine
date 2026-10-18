/**
 * Splits the text of a `text/event-stream` body, as it arrives in chunks, into frames:
 * `event` and `data` lines up to a blank line. The graph stream uses the browser's `EventSource`;
 * the audit's answer is a POST, so its stream is read by hand.
 */
export interface SseFrame {
  event: string;
  data: string;
}

export class SseFrameParser {
  private buffer = '';
  private event = 'message';
  private data: string[] = [];

  /** Feeds a chunk; returns the frames it completed. */
  push(chunk: string): SseFrame[] {
    this.buffer += chunk;
    const frames: SseFrame[] = [];
    let newline = this.buffer.indexOf('\n');
    while (newline !== -1) {
      const line = this.buffer.slice(0, newline).replace(/\r$/, '');
      this.buffer = this.buffer.slice(newline + 1);
      const frame = this.line(line);
      if (frame) frames.push(frame);
      newline = this.buffer.indexOf('\n');
    }
    return frames;
  }

  private line(line: string): SseFrame | null {
    if (line === '') {
      if (this.data.length === 0) return null;
      const frame = { event: this.event, data: this.data.join('\n') };
      this.event = 'message';
      this.data = [];
      return frame;
    }
    if (line.startsWith(':')) return null;
    const colon = line.indexOf(':');
    const field = colon === -1 ? line : line.slice(0, colon);
    const value = colon === -1 ? '' : line.slice(colon + 1).replace(/^ /, '');
    if (field === 'event') this.event = value;
    else if (field === 'data') this.data.push(value);
    return null;
  }
}
