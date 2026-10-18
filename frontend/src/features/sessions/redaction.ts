/** A piece of recorded text: as written, or a marker the recording put in its place. */
export interface TextPiece {
  text: string;
  /** A redaction (`[redacted restricted passage …]`, `[redacted secret]`) or a cut. */
  marker: boolean;
}

const MARKER = /\[(?:redacted [^\]]*|… cut:[^\]]*)\]/g;

/** Splits recorded text into its pieces, so the markers can be shown as such. */
export function splitMarkers(content: string): TextPiece[] {
  const pieces: TextPiece[] = [];
  let last = 0;
  for (const match of content.matchAll(MARKER)) {
    const at = match.index;
    if (at > last) pieces.push({ text: content.slice(last, at), marker: false });
    pieces.push({ text: match[0], marker: true });
    last = at + match[0].length;
  }
  if (last < content.length) pieces.push({ text: content.slice(last), marker: false });
  return pieces;
}

/** How many markers a text holds. */
export function countMarkers(content: string): number {
  return content.match(MARKER)?.length ?? 0;
}
