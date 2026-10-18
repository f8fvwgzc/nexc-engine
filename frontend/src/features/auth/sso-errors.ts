/**
 * What the login page says when single sign-on sent the browser back without a session
 * (`/login?sso_error=<code>`, CONTRACT §5). `provider` is the name on the button.
 */
export function ssoErrorMessage(code: string, provider: string): string {
  switch (code) {
    case 'unavailable':
      return `${provider} could not be reached. Try again in a moment.`;
    case 'denied':
      return `The sign-in was cancelled at ${provider}.`;
    case 'expired':
      return 'That sign-in took too long or was already used. Start again.';
    case 'invalid':
      return `What ${provider} sent back did not check out. Start again; if it keeps happening, tell whoever administers this installation.`;
    case 'no_email':
      return `${provider} did not share an e-mail address, which an account here needs.`;
    case 'unverified_email':
      return `${provider} has not verified your e-mail address, so it cannot sign you in here.`;
    case 'email_taken':
      return `An account with your e-mail address is linked to another ${provider} identity. Ask whoever administers this installation.`;
    case 'suspended':
      return 'This account is suspended; contact whoever administers this installation.';
    default:
      return `Signing in with ${provider} did not work. Try again.`;
  }
}
