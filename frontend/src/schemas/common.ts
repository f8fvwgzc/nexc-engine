import { z } from 'zod';

/** Ids are UUID v7 strings (CONTRACT §3). */
export const idSchema = z.uuid();

/** RFC 3339 timestamps; the backend emits UTC but we accept any explicit offset. */
export const timestampSchema = z.iso.datetime({ offset: true });

export const nullableTimestampSchema = timestampSchema.nullable();
