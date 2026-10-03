import { sseEventSchemas, type SseEvent, type SseEventType } from '@/schemas/realtime';

function isKnownType(type: string): type is SseEventType {
  return Object.hasOwn(sseEventSchemas, type);
}

/**
 * Validates one SSE frame (event name + raw `data`) against CONTRACT §6.
 * Returns null for unknown event names, invalid JSON or payloads that fail the schema.
 */
export function parseSseEvent(
  type: string,
  raw: string,
  id: string | null = null,
): SseEvent | null {
  if (!isKnownType(type)) return null;
  let json: unknown;
  try {
    json = JSON.parse(raw);
  } catch {
    return null;
  }
  const result = sseEventSchemas[type].safeParse(json);
  if (!result.success) return null;
  return { type, id, data: result.data } as SseEvent;
}
