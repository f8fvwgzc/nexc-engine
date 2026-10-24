import { render, screen } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import { ScrollRegion } from './scroll-region';

/** A region holding `count` columns, each 300 px wide, in a box 500 px wide. */
function columns(count: number) {
  return (
    <ScrollRegion label="Board" measureKey={count}>
      {Array.from({ length: count }, (_, i) => (
        <div key={i} />
      ))}
    </ScrollRegion>
  );
}

describe('ScrollRegion', () => {
  beforeEach(() => {
    // jsdom lays nothing out: the box is 500 px wide and its content 300 px per child.
    vi.spyOn(Element.prototype, 'clientWidth', 'get').mockReturnValue(500);
    vi.spyOn(Element.prototype, 'scrollWidth', 'get').mockImplementation(function (this: Element) {
      return this.children.length * 300;
    });
  });

  it('is a plain box while nothing overflows', () => {
    render(columns(1));
    expect(screen.queryByRole('group', { name: 'Board' })).not.toBeInTheDocument();
  });

  it('becomes a focusable region once content added later overflows, in a box of the same size', () => {
    const view = render(columns(1));
    view.rerender(columns(3));
    const region = screen.getByRole('group', { name: 'Board' });
    expect(region).toHaveAttribute('tabindex', '0');

    // And a plain box again once the content fits.
    view.rerender(columns(1));
    expect(screen.queryByRole('group', { name: 'Board' })).not.toBeInTheDocument();
  });
});
