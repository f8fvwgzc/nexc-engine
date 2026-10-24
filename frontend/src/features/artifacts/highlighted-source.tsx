import { CodeBlockLine } from '@/components/ui/code-block';
import { useHighlightedCode } from '@/hooks/use-highlighted-code';

/**
 * A file's lines, coloured by shiki for its language. Loaded on demand (shiki and each grammar
 * are their own chunks); until a grammar arrives, or for a language shiki does not know, the
 * lines show plain.
 */
export default function HighlightedSource({
  content,
  language,
}: {
  content: string;
  language: string;
}) {
  const lines = useHighlightedCode(content, language);
  return lines.map((line, i) => <CodeBlockLine key={i}>{line}</CodeBlockLine>);
}
