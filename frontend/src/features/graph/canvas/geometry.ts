export const NODE_WIDTH = 184;
export const NODE_HEIGHT = 52;

export interface Point {
  x: number;
  y: number;
}

/** Point where the ray from the center of a node toward `toward` leaves its (padded) card. */
export function clipToCard(center: Point, toward: Point, padding = 4): Point {
  const dx = toward.x - center.x;
  const dy = toward.y - center.y;
  if (dx === 0 && dy === 0) return center;
  const hw = NODE_WIDTH / 2 + padding;
  const hh = NODE_HEIGHT / 2 + padding;
  const t = Math.min(
    dx === 0 ? Infinity : hw / Math.abs(dx),
    dy === 0 ? Infinity : hh / Math.abs(dy),
  );
  return { x: center.x + dx * Math.min(t, 1), y: center.y + dy * Math.min(t, 1) };
}

/**
 * Gently curved edge between two node centers, trimmed to the card borders so the arrowhead sits
 * on the target's edge. `bend` is the perpendicular offset as a fraction of the distance.
 */
export function edgePath(source: Point, target: Point, bend = 0.12): string {
  const mx = (source.x + target.x) / 2;
  const my = (source.y + target.y) / 2;
  const dx = target.x - source.x;
  const dy = target.y - source.y;
  const control = { x: mx - dy * bend, y: my + dx * bend };
  const start = clipToCard(source, control);
  const end = clipToCard(target, control, 7);
  return `M${r(start.x)},${r(start.y)}Q${r(control.x)},${r(control.y)} ${r(end.x)},${r(end.y)}`;
}

/** Straight segment from a node border to a free point (connect-drag preview). */
export function previewPath(source: Point, pointer: Point): string {
  const start = clipToCard(source, pointer);
  return `M${r(start.x)},${r(start.y)}L${r(pointer.x)},${r(pointer.y)}`;
}

function r(n: number): number {
  return Math.round(n * 10) / 10;
}

export function truncate(text: string, max: number): string {
  return text.length > max ? `${text.slice(0, max - 1).trimEnd()}…` : text;
}
