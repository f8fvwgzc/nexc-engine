import { SparklesIcon } from 'lucide-react';
import { useState } from 'react';

import { AnimatedButton } from '@/components/custom-ui/animated-button';
import { Button } from '@/components/ui/button';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import { Label } from '@/components/ui/label';
import { Textarea } from '@/components/ui/textarea';
import type { GraphTemplate } from '@/schemas/template';

/** Example topics per template category, shown as the placeholder. */
const EXAMPLES: Record<string, string> = {
  research: 'e.g. The state of solid-state batteries for EVs in 2026',
  engineering: 'e.g. A bookmarks API with tags, search and per-user auth',
  business: 'e.g. Should we launch a B2B plant-care subscription in Germany?',
  writing: 'e.g. A beginner-friendly series on Rust async',
  data: 'e.g. Daily Stripe + Postgres data into a BigQuery revenue mart',
  marketing: 'e.g. Launching a privacy-first note-taking app for students',
};

interface TemplateTopicDialogProps {
  template: GraphTemplate | null;
  pending: boolean;
  onOpenChange: (open: boolean) => void;
  onCreate: (topic: string) => void;
}

/**
 * Asks what a template instance is about before creating it: templates are generic, and without
 * a topic the agents have nothing concrete to work on.
 */
export function TemplateTopicDialog({
  template,
  pending,
  onOpenChange,
  onCreate,
}: TemplateTopicDialogProps) {
  const [topic, setTopic] = useState('');
  const trimmed = topic.trim();

  return (
    <Dialog
      open={template !== null}
      onOpenChange={(open) => {
        if (!open) setTopic('');
        onOpenChange(open);
      }}
    >
      <DialogContent className="sm:max-w-lg">
        <form
          className="space-y-4"
          onSubmit={(e) => {
            e.preventDefault();
            if (trimmed) onCreate(trimmed);
          }}
        >
          <DialogHeader>
            <DialogTitle>{template?.name}</DialogTitle>
            <DialogDescription>
              What should this graph work on? Every planner and agent prompt starts from it.
            </DialogDescription>
          </DialogHeader>
          <div className="space-y-1.5">
            <Label htmlFor="template-topic">Topic</Label>
            <Textarea
              id="template-topic"
              rows={3}
              autoFocus
              required
              maxLength={2000}
              value={topic}
              placeholder={EXAMPLES[template?.category ?? ''] ?? 'Describe the subject or goal'}
              onChange={(e) => setTopic(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === 'Enter' && (e.metaKey || e.ctrlKey) && trimmed) onCreate(trimmed);
              }}
            />
          </div>
          <DialogFooter>
            <Button type="button" variant="ghost" onClick={() => onOpenChange(false)}>
              Cancel
            </Button>
            <AnimatedButton
              type="submit"
              glow
              disabled={!trimmed}
              loading={pending}
              loadingText="Creating…"
            >
              <SparklesIcon />
              Create graph
            </AnimatedButton>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
