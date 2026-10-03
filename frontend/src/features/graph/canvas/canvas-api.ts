import type { Point } from './geometry';

/** Imperative handle exposed by the canvas to the toolbar / keyboard shortcuts. */
export interface CanvasApi {
  fitToView: (animate?: boolean) => void;
  /** Center of the visible area in graph coordinates (where "Add node" drops a node). */
  viewportCenter: () => Point;
}
