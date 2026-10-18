import { useEffect, useState, type RefObject } from 'react';

/**
 * Whether the element's content is larger than the element along `axis`, so it scrolls. A region
 * that scrolls must be reachable by keyboard (WCAG 2.1.1): callers make it focusable while it does.
 */
export function useOverflow(
  ref: RefObject<HTMLElement | null>,
  axis: 'x' | 'y' | 'both' = 'x',
): boolean {
  const [overflows, setOverflows] = useState(false);
  useEffect(() => {
    const element = ref.current;
    if (!element || typeof ResizeObserver === 'undefined') return;
    const measure = () => {
      const x = element.scrollWidth > element.clientWidth + 1;
      const y = element.scrollHeight > element.clientHeight + 1;
      setOverflows(axis === 'x' ? x : axis === 'y' ? y : x || y);
    };
    const observer = new ResizeObserver(measure);
    observer.observe(element);
    for (const child of element.children) observer.observe(child);
    measure();
    return () => observer.disconnect();
  }, [ref, axis]);
  return overflows;
}

/** {@link useOverflow} sideways: whether a table or a chart scrolls horizontally. */
export function useOverflowX(ref: RefObject<HTMLElement | null>): boolean {
  return useOverflow(ref, 'x');
}
