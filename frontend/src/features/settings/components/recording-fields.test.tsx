import { fireEvent, render, screen } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';

import { RECORDING_OFF, type Recording } from '@/schemas/workspace';

import { RecordingFields } from './recording-fields';

describe('RecordingFields', () => {
  it('switches kinds on, keeps the retention and the labels', () => {
    const onChange = vi.fn<(next: Recording) => void>();
    render(<RecordingFields value={RECORDING_OFF} onChange={onChange} />);
    // Off: the retention and the labels wait for a kind to be switched on.
    expect(screen.getByRole('textbox')).toBeDisabled();
    expect(screen.getByRole('checkbox', { name: 'Redact restricted passages' })).toBeDisabled();
    fireEvent.click(screen.getByRole('switch', { name: 'Record prompts' }));
    expect(onChange).toHaveBeenLastCalledWith({ ...RECORDING_OFF, prompts: true });
  });

  it('edits the retention and the redacted labels once a kind is on', () => {
    const onChange = vi.fn<(next: Recording) => void>();
    const on: Recording = { ...RECORDING_OFF, completions: true, redact_labels: ['confidential'] };
    render(<RecordingFields value={on} onChange={onChange} />);
    const retention = screen.getByRole('textbox');
    expect(retention).toBeEnabled();
    fireEvent.change(retention, { target: { value: '9x0' } });
    expect(onChange).toHaveBeenLastCalledWith({ ...on, retention_days: 90 });
    fireEvent.click(screen.getByRole('checkbox', { name: 'Redact restricted passages' }));
    expect(onChange).toHaveBeenLastCalledWith({
      ...on,
      redact_labels: ['confidential', 'restricted'],
    });
    fireEvent.click(screen.getByRole('checkbox', { name: 'Redact confidential passages' }));
    expect(onChange).toHaveBeenLastCalledWith({ ...on, redact_labels: [] });
  });
});
