import { useQuery } from '@tanstack/react-query';
import {
  BotIcon,
  BrainIcon,
  EyeOffIcon,
  FileTextIcon,
  GlobeIcon,
  PlayIcon,
  SearchCheckIcon,
  SendHorizontalIcon,
  ShieldIcon,
  WrenchIcon,
  type LucideIcon,
} from 'lucide-react';
import { useEffect, useId, useRef, useState } from 'react';
import { Link } from 'react-router-dom';

import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { errorMessage } from '@/lib/api/errors';
import { formatTokens } from '@/lib/format';
import { cn } from '@/lib/utils';
import type { AuditAnswer, AuditFact, CannedQuestion } from '@/schemas/audit';

import { askAuditStream, auditQuestionsQuery } from '../api';

const FACT_ICON: Record<string, LucideIcon> = {
  agent: BotIcon,
  run: PlayIcon,
  node: PlayIcon,
  document: FileTextIcon,
  memory: BrainIcon,
  policy: ShieldIcon,
  action: ShieldIcon,
  tool: WrenchIcon,
  host: GlobeIcon,
  model: BrainIcon,
  withheld: EyeOffIcon,
};

function latency(ms: number): string {
  if (ms < 10) return `${ms.toFixed(2)} ms`;
  if (ms < 1000) return `${Math.round(ms)} ms`;
  return `${(ms / 1000).toFixed(1)} s`;
}

/** How the answer was produced: green and free when the graph alone answered, neutral for a model. */
function ModeBadge({ answer }: { answer: AuditAnswer }) {
  const detail = `${formatTokens(answer.tokens)} tokens · ${latency(answer.latency_ms)}`;
  if (answer.mode === 'deterministic') {
    return (
      <span
        data-mode="deterministic"
        style={{ '--badge-color': 'var(--status-succeeded)' } as React.CSSProperties}
        className={cn(
          'inline-flex h-5 items-center gap-1.5 rounded-full px-2 text-[11px] font-medium text-badge',
          'bg-[color-mix(in_oklch,var(--badge-color)_12%,transparent)]',
        )}
      >
        <span className="size-1.5 rounded-full bg-[var(--badge-color)]" />
        deterministic · {detail}
      </span>
    );
  }
  return (
    <Badge variant="outline" data-mode="llm" className="text-[11px]">
      llm · {detail}
    </Badge>
  );
}

function Fact({ fact }: { fact: AuditFact }) {
  const Icon = FACT_ICON[fact.kind] ?? SearchCheckIcon;
  const withheld = fact.kind === 'withheld';
  const body = (
    <>
      <Icon className="size-3.5 shrink-0" aria-hidden />
      <span className="truncate">{fact.label}</span>
    </>
  );
  const className = cn(
    'inline-flex max-w-full items-center gap-1.5 rounded-full border px-2 py-0.5 text-xs',
    withheld && 'border-dashed text-muted-foreground',
  );
  if (fact.link && !withheld) {
    return (
      <li>
        <Link
          to={fact.link}
          title={fact.id}
          className={cn(className, 'hover:bg-muted hover:text-foreground')}
        >
          {body}
        </Link>
      </li>
    );
  }
  return (
    <li>
      <span
        title={withheld ? 'Not in your clearance' : fact.id}
        className={className}
        data-withheld={withheld || undefined}
      >
        {body}
      </span>
    </li>
  );
}

function Answer({ answer }: { answer: AuditAnswer }) {
  return (
    <div
      className="space-y-2.5"
      data-testid="audit-answer"
      data-mode={answer.mode}
      data-unverified={answer.unverified}
    >
      <div className="flex flex-wrap items-center gap-2">
        <ModeBadge answer={answer} />
        {answer.unverified && (
          <Badge variant="destructive" className="text-[11px]">
            Unverified
          </Badge>
        )}
      </div>
      {answer.unverified && (
        <p
          role="status"
          className="rounded-lg border border-destructive/30 bg-destructive/10 px-3 py-2 text-xs text-destructive"
        >
          Unverified: this answer cites no fact. Read it as the model's words, not the record.
        </p>
      )}
      <p className="text-sm whitespace-pre-wrap">{answer.answer}</p>
      {answer.facts.length > 0 && (
        <ul aria-label="Facts" className="flex flex-wrap gap-1.5">
          {answer.facts.map((fact) => (
            <Fact key={fact.id} fact={fact} />
          ))}
        </ul>
      )}
    </div>
  );
}

/** A chip's text with its slots filled, or `null` when a slot has nothing to fill it. */
function fill(label: string, slots: { run?: string; agent?: string }): string | null {
  let text = label;
  for (const [slot, value] of [
    ['{run}', slots.run],
    ['{agent}', slots.agent],
  ] as const) {
    if (!text.includes(slot)) continue;
    if (!value) return null;
    text = text.replace(slot, value);
  }
  return text;
}

/** The chip as shown: slots the panel cannot fill read as "…". */
function chipLabel(label: string): string {
  return label.replace('{run}', '…').replace('{agent}', '…');
}

export interface AskAuditPanelProps {
  workspaceId: string;
  /** Fills the `{agent}` slot of the chips (an agent's card). */
  agentName?: string;
  /** Fills the `{run}` slot of the chips (a run's page). */
  runId?: string;
  /** Tighter spacing, for a side panel. */
  compact?: boolean;
  className?: string;
}

/**
 * Ask the audit: six canned questions as chips and a box for any other. The answer comes
 * with how it was produced (from the graph, 0 tokens, in milliseconds; or a model's
 * explanation), the facts it rests on as links into nexc, and an "unverified" mark when it
 * cites none.
 */
export function AskAuditPanel({
  workspaceId,
  agentName,
  runId,
  compact = false,
  className,
}: AskAuditPanelProps) {
  const { data: questions = [] } = useQuery(auditQuestionsQuery(workspaceId));
  const inputId = useId();
  const inputRef = useRef<HTMLInputElement>(null);
  const abortRef = useRef<AbortController | null>(null);
  const [question, setQuestion] = useState('');
  const [asked, setAsked] = useState<string | null>(null);
  const [pending, setPending] = useState(false);
  const [stage, setStage] = useState<string | null>(null);
  const [draft, setDraft] = useState('');
  const [answer, setAnswer] = useState<AuditAnswer | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => () => abortRef.current?.abort(), []);

  const ask = async (text: string) => {
    const trimmed = text.trim();
    if (!trimmed || pending) return;
    abortRef.current?.abort();
    const abort = new AbortController();
    abortRef.current = abort;
    setAsked(trimmed);
    setPending(true);
    setStage(null);
    setDraft('');
    setAnswer(null);
    setError(null);
    try {
      await askAuditStream(
        workspaceId,
        trimmed,
        (event) => {
          if (event.type === 'stage') setStage(event.text);
          else if (event.type === 'delta') setDraft((d) => d + event.text);
          else if (event.type === 'done') setAnswer(event.answer);
          else setError(event.detail);
        },
        abort.signal,
      );
    } catch (err) {
      if (!abort.signal.aborted) setError(errorMessage(err));
    } finally {
      if (abortRef.current === abort) setPending(false);
    }
  };

  const pick = (chip: CannedQuestion) => {
    const filled = fill(chip.label, { run: runId, agent: agentName });
    if (filled) {
      setQuestion(filled);
      void ask(filled);
      return;
    }
    // A slot nobody can fill here: hand the asker the start of the question.
    const slot = chip.label.indexOf('{');
    setQuestion(chip.label.slice(0, slot));
    inputRef.current?.focus();
  };

  // Compact, it sits in a console panel that is already the "Ask the audit" region.
  const Frame = compact ? 'div' : 'section';
  return (
    <Frame
      aria-label={compact ? undefined : 'Ask the audit'}
      className={cn('space-y-3 rounded-lg border p-4', compact && 'p-3', className)}
    >
      {!compact && (
        <div className="space-y-1">
          <h2 className="text-[13px] font-medium">Ask the audit</h2>
          <p className="text-[13px] text-muted-foreground">
            What agents read, called, were refused and were approved for, answered from the record.
            The canned questions are answered from the graph alone; others may be explained by a
            model, which always cites what it was given.
          </p>
        </div>
      )}
      <ul aria-label="Questions" className="flex flex-wrap gap-1.5">
        {questions.map((chip) => (
          <li key={chip.id}>
            <Button
              type="button"
              variant="outline"
              size="xs"
              className="h-auto rounded-full py-1 whitespace-normal"
              disabled={pending}
              onClick={() => pick(chip)}
            >
              {chipLabel(fill(chip.label, { run: runId, agent: agentName }) ?? chip.label)}
            </Button>
          </li>
        ))}
      </ul>
      <form
        className="flex gap-2"
        onSubmit={(e) => {
          e.preventDefault();
          void ask(question);
        }}
      >
        <label htmlFor={inputId} className="sr-only">
          Question
        </label>
        <Input
          id={inputId}
          ref={inputRef}
          value={question}
          maxLength={1000}
          placeholder="Ask anything about what agents did…"
          autoComplete="off"
          className="h-8 text-sm"
          onChange={(e) => setQuestion(e.target.value)}
        />
        <Button type="submit" size="sm" disabled={pending || !question.trim()}>
          <SendHorizontalIcon />
          Ask
        </Button>
      </form>
      {asked && (
        <div className="space-y-2 border-t pt-3" aria-live="polite">
          <p className="text-xs text-muted-foreground">
            <span className="font-medium text-foreground">Q.</span> {asked}
          </p>
          {error ? (
            <p role="alert" className="text-sm text-destructive">
              {error}
            </p>
          ) : answer ? (
            <Answer answer={answer} />
          ) : (
            <div className="space-y-1.5" aria-busy>
              <p className="text-xs text-muted-foreground">
                {stage ? `${stage}…` : 'Asking the audit…'}
              </p>
              {draft && <p className="text-sm whitespace-pre-wrap">{draft}</p>}
            </div>
          )}
        </div>
      )}
    </Frame>
  );
}
