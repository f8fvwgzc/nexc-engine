import { easeCubicInOut, interpolateZoom, zoomIdentity, type ZoomTransform } from 'd3';

/**
 * Smoothly animates between two zoom transforms (van Wijk & Nuij "smooth zoom") without pulling
 * in d3-transition. Returns a cancel function.
 */
export function animateZoom(
  from: ZoomTransform,
  to: ZoomTransform,
  size: { width: number; height: number },
  apply: (t: ZoomTransform) => void,
): () => void {
  const { width, height } = size;
  const view = (t: ZoomTransform): [number, number, number] => [
    (width / 2 - t.x) / t.k,
    (height / 2 - t.y) / t.k,
    width / t.k,
  ];
  const interpolate = interpolateZoom(view(from), view(to));
  const duration = Math.min(900, Math.max(320, interpolate.duration * 0.6));
  const start = performance.now();
  let frame = 0;

  const step = (now: number) => {
    const progress = Math.min(1, (now - start) / duration);
    const [cx, cy, w] = interpolate(easeCubicInOut(progress));
    const k = width / w;
    apply(zoomIdentity.translate(width / 2 - cx * k, height / 2 - cy * k).scale(k));
    if (progress < 1) frame = requestAnimationFrame(step);
  };
  frame = requestAnimationFrame(step);
  return () => cancelAnimationFrame(frame);
}
