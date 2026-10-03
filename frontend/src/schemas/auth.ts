import { z } from 'zod';

import { idSchema, timestampSchema } from './common';

export const roleSchema = z.enum(['admin', 'user']);
export type Role = z.infer<typeof roleSchema>;

export const userSchema = z.object({
  id: idSchema,
  email: z.email(),
  name: z.string(),
  role: roleSchema,
  created_at: timestampSchema,
});
export type User = z.infer<typeof userSchema>;

export const authResponseSchema = z.object({
  user: userSchema,
  access_token: z.string().min(1),
  expires_in: z.number().int().positive(),
});
export type AuthResponse = z.infer<typeof authResponseSchema>;

/* ---------- request bodies / form schemas ---------- */

export const loginInputSchema = z.object({
  email: z.email({ error: 'Enter a valid email address' }),
  password: z.string().min(1, { error: 'Password is required' }),
});
export type LoginInput = z.infer<typeof loginInputSchema>;

export const PASSWORD_MIN_LENGTH = 12;

export const registerInputSchema = z.object({
  name: z
    .string()
    .trim()
    .min(1, { error: 'Tell us what to call you' })
    .max(100, { error: 'Name must be at most 100 characters' }),
  email: z.email({ error: 'Enter a valid email address' }),
  password: z
    .string()
    .min(PASSWORD_MIN_LENGTH, { error: `Use at least ${PASSWORD_MIN_LENGTH} characters` })
    .max(256, { error: 'Password is too long' }),
});
export type RegisterInput = z.infer<typeof registerInputSchema>;
