import { z } from 'zod';

import { idSchema, nullableTimestampSchema, timestampSchema } from './common';
import { alertKindSchema, severitySchema } from './policy';

/* ---------- alert routing (CONTRACT §4) ---------- */

export const DESTINATION_KINDS = ['webhook', 'slack'] as const;
export const destinationKindSchema = z.enum(DESTINATION_KINDS);
export type DestinationKind = z.infer<typeof destinationKindSchema>;

export const deliveryStatusSchema = z.enum(['pending', 'delivered', 'failed']);
export type DeliveryStatus = z.infer<typeof deliveryStatusSchema>;

export const deliveryEventSchema = z.enum([
  'alert.raised',
  'approval.requested',
  'approval.decided',
  'test',
]);
export type DeliveryEvent = z.infer<typeof deliveryEventSchema>;

/** Where a workspace's alerts and approvals go; the URL's host only, the secret never. */
export const alertDestinationSchema = z.object({
  id: idSchema,
  workspace_id: idSchema,
  name: z.string(),
  kind: destinationKindSchema,
  host: z.string(),
  /** Alert kinds sent; empty means every kind. */
  kinds: z.array(alertKindSchema),
  min_severity: severitySchema,
  include_approvals: z.boolean(),
  enabled: z.boolean(),
  /** Why the platform switched it off, when it did. */
  disabled_reason: z.string(),
  /** Deliveries that failed in a row; at 50 the destination is switched off. */
  failures: z.number().int(),
  last_delivered_at: nullableTimestampSchema,
  last_error: z.string(),
  created_by: idSchema.nullable(),
  created_at: timestampSchema,
  updated_at: timestampSchema,
});
export type AlertDestination = z.infer<typeof alertDestinationSchema>;

/** The answer to a registration: the destination and its signing secret, shown once. */
export const issuedAlertDestinationSchema = z.object({
  destination: alertDestinationSchema,
  secret: z.string(),
});
export type IssuedAlertDestination = z.infer<typeof issuedAlertDestinationSchema>;

/** One entry of a destination's delivery log. */
export const alertDeliverySchema = z.object({
  id: idSchema,
  destination_id: idSchema,
  event: deliveryEventSchema,
  alert_id: idSchema.nullable(),
  approval_id: idSchema.nullable(),
  status: deliveryStatusSchema,
  attempts: z.number().int(),
  next_attempt_at: nullableTimestampSchema,
  last_attempt_at: nullableTimestampSchema,
  last_status: z.number().int().nullable(),
  last_error: z.string(),
  created_at: timestampSchema,
  delivered_at: nullableTimestampSchema,
});
export type AlertDelivery = z.infer<typeof alertDeliverySchema>;

/** What `POST …/test` found. */
export const destinationTestSchema = z.object({
  ok: z.boolean(),
  status: z.number().int().nullable(),
  detail: z.string(),
  latency_ms: z.number(),
});
export type DestinationTest = z.infer<typeof destinationTestSchema>;

/** The registration form. */
export const alertDestinationInputSchema = z.object({
  name: z.string().trim().min(1, { error: 'Name is required' }).max(100),
  kind: destinationKindSchema,
  url: z
    .string()
    .trim()
    .min(1, { error: 'URL is required' })
    .max(2000)
    .refine((url) => /^https?:\/\/\S+$/i.test(url), { error: 'Must be an http(s) URL' }),
  /** Empty sends every kind. */
  kinds: z.array(alertKindSchema),
  min_severity: severitySchema,
  include_approvals: z.boolean(),
});
export type AlertDestinationInput = z.infer<typeof alertDestinationInputSchema>;

export const EMPTY_ALERT_DESTINATION: AlertDestinationInput = {
  name: '',
  kind: 'webhook',
  url: '',
  kinds: [],
  min_severity: 'low',
  include_approvals: false,
};

/** What `PATCH` changes; a field left out stays. */
export interface AlertDestinationPatch {
  name?: string;
  url?: string;
  kinds?: AlertDestination['kinds'];
  min_severity?: AlertDestination['min_severity'];
  include_approvals?: boolean;
  enabled?: boolean;
}
