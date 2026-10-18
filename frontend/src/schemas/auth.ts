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

/** A single sign-on provider the login page offers (CONTRACT §5, `GET /auth/providers`). */
export const ssoProviderSchema = z.object({
  id: z.string(),
  display_name: z.string(),
  login_url: z.string(),
});
export type SsoProvider = z.infer<typeof ssoProviderSchema>;

/** The ways to sign in to this installation. */
export const loginProvidersSchema = z.object({
  /** False when only single sign-on is allowed: the password form is hidden. */
  password: z.boolean(),
  signup: z.boolean(),
  sso: z.array(ssoProviderSchema),
});
export type LoginProviders = z.infer<typeof loginProvidersSchema>;

/** A single sign-on identity linked to the signed-in account. */
export const linkedIdentitySchema = z.object({
  provider: z.string(),
  issuer: z.string(),
  email: z.string(),
  linked_at: timestampSchema,
  last_login_at: timestampSchema.nullable(),
});
export type LinkedIdentity = z.infer<typeof linkedIdentitySchema>;

/** How the signed-in account signs in (`GET /auth/identities`). */
export const signInMethodsSchema = z.object({
  password: z.boolean(),
  identities: z.array(linkedIdentitySchema),
});
export type SignInMethods = z.infer<typeof signInMethodsSchema>;

/* ---------- request bodies / form schemas ---------- */

export const loginInputSchema = z.object({
  email: z.email({ error: 'Enter a valid email address' }),
  password: z.string().min(1, { error: 'Password is required' }),
  /** For an account with two-factor sign-in: the app's code or a recovery code. */
  code: z.string().optional(),
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
