import { toast } from 'sonner';

/** Puts `text` on the clipboard and says so (`done`), or that the clipboard is not there. */
export async function copyText(text: string, done: string) {
  try {
    await navigator.clipboard.writeText(text);
    toast.success(done);
  } catch {
    toast.error('Clipboard is not available');
  }
}
