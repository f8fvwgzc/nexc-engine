import { render, screen } from '@testing-library/react';
import { describe, expect, it } from 'vitest';

import { Markdown } from './markdown';

describe('Markdown', () => {
  it('renders GitHub-flavoured markdown', () => {
    render(
      <Markdown>{'## Findings\n\n**Revenue** grew.\n\n| a | b |\n|---|---|\n| 1 | 2 |'}</Markdown>,
    );
    expect(screen.getByRole('heading', { name: 'Findings' })).toBeInTheDocument();
    expect(screen.getByText('Revenue').tagName).toBe('STRONG');
    expect(screen.getByRole('table')).toBeInTheDocument();
  });

  it('never renders raw HTML or remote images from untrusted output', () => {
    const { container } = render(
      <Markdown>
        {'<script>alert(1)</script><b>raw</b>\n\n![pixel](https://evil.example/p.png)'}
      </Markdown>,
    );
    expect(container.querySelector('script')).toBeNull();
    expect(container.querySelector('b')).toBeNull();
    expect(container.querySelector('img')).toBeNull();
  });

  it('opens links in a new tab without an opener', () => {
    render(<Markdown>{'[docs](https://example.com)'}</Markdown>);
    const link = screen.getByRole('link', { name: 'docs' });
    expect(link).toHaveAttribute('target', '_blank');
    expect(link).toHaveAttribute('rel', 'noopener noreferrer nofollow');
  });
});
