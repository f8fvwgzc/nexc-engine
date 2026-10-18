import type MarkdownIt from 'markdown-it';

/**
 * Renders a ```mermaid fence as the `Mermaid` component (`theme/Mermaid.vue`), which draws the
 * diagram in the browser with the site's light or dark theme. The source travels URL-encoded
 * in an attribute so that no character of it reaches the Vue template compiler.
 */
export function mermaidFence(md: MarkdownIt): void {
  const fence = md.renderer.rules.fence!;
  md.renderer.rules.fence = (tokens, idx, options, env, self) => {
    const token = tokens[idx];
    if (token.info.trim().split(/\s+/)[0] === 'mermaid') {
      return `<Mermaid code="${encodeURIComponent(token.content)}" />\n`;
    }
    return fence(tokens, idx, options, env, self);
  };
}
