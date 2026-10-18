import { fireEvent, render, screen } from '@testing-library/react';
import { describe, expect, it } from 'vitest';

import type { SpanPayload } from '@/schemas/session';

import { countMarkers, splitMarkers } from '../redaction';
import { PayloadViewer } from './payload-viewer';

const chunk = '0190a1b2-c3d4-7e5f-8a9b-2c1d2e3f4a5b';
const payload: SpanPayload = {
  kind: 'prompt',
  content: `[system]\nYou research.\n\n<document>\n[redacted restricted passage ${chunk}]\n</document>\n\nKEY=[redacted secret] done`,
  size: 120,
  redactions: 2,
  created_at: '2026-10-18T12:00:00Z',
};

describe('PayloadViewer', () => {
  it('shows the text in monospace with the redactions marked, copies and folds', () => {
    render(<PayloadViewer payload={payload} />);
    const section = screen.getByRole('region', { name: 'Prompt' });
    expect(section).toHaveAttribute('data-kind', 'prompt');
    const marks = section.querySelectorAll('mark[data-marker]');
    expect(marks).toHaveLength(2);
    expect(marks[0]).toHaveTextContent(`[redacted restricted passage ${chunk}]`);
    expect(marks[1]).toHaveTextContent('[redacted secret]');
    expect(screen.getByText('2 redacted')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Copy the prompt' })).toBeInTheDocument();
    const pre = section.querySelector('pre')!;
    expect(pre).toHaveClass('font-mono');
    expect(pre).not.toHaveAttribute('hidden');
    fireEvent.click(screen.getByRole('button', { name: 'Fold the payload' }));
    expect(pre).toHaveAttribute('hidden');
    expect(screen.getByRole('button', { name: 'Unfold the payload' })).toHaveAttribute(
      'aria-expanded',
      'false',
    );
  });

  it('opens a long payload folded', () => {
    render(<PayloadViewer payload={{ ...payload, kind: 'tool_output', size: 100_000 }} />);
    expect(
      screen.getByRole('region', { name: 'Tool output' }).querySelector('pre'),
    ).toHaveAttribute('hidden');
  });

  it('splits markers out of the text', () => {
    expect(splitMarkers('a [redacted secret] b [… cut: 9 characters in all]')).toEqual([
      { text: 'a ', marker: false },
      { text: '[redacted secret]', marker: true },
      { text: ' b ', marker: false },
      { text: '[… cut: 9 characters in all]', marker: true },
    ]);
    expect(splitMarkers('plain')).toEqual([{ text: 'plain', marker: false }]);
    expect(countMarkers('[redacted x] [redacted y]')).toBe(2);
    expect(countMarkers('none')).toBe(0);
  });
});
