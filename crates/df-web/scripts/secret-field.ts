// Alpine component of the "Incoming webhook" group (`x-data='secretField("<secret url>", "<webhook url>", "<mask>")'`,
// see `project_webhook.rs`). The page holds only the mask; the secret is fetched when an admin shows or
// copies it.
//
// State: `shown` and `secret` (empty while masked: hiding drops the value), `copied` (`'url'` or
// `'secret'` for 2 s after a copy), `error` (shown at the field), `canCopy` (the browser offers the
// asynchronous clipboard API, which needs a secure context: HTTPS or a loopback host).
//
// `toggle()`: shows (fetches once) or hides the secret. `copy(what)`: copies the webhook URL or the
// secret; a masked secret is fetched, copied and dropped again without being shown.
const COPIED_MS = 2000;

document.addEventListener('alpine:init', () => {
  Alpine.data('secretField', (secretUrl: string, webhookUrl: string, mask: string) => ({
    mask,
    shown: false,
    secret: '',
    copied: '',
    error: '',
    canCopy: window.isSecureContext && Boolean(navigator.clipboard),
    timer: 0,

    async fetchSecret(): Promise<string> {
      const response = await fetch(secretUrl, { headers: { Accept: 'text/plain' }, cache: 'no-store' });
      if (!response.ok) throw new Error(`HTTP ${response.status}`);
      return response.text();
    },

    async toggle(): Promise<void> {
      this.error = '';
      if (this.shown) {
        this.shown = false;
        this.secret = '';
        return;
      }
      try {
        this.secret = await this.fetchSecret();
        this.shown = true;
      } catch {
        this.error = 'Could not read the secret.';
      }
    },

    async copy(what: string): Promise<void> {
      this.error = '';
      try {
        const value = what === 'url' ? webhookUrl : this.shown ? this.secret : await this.fetchSecret();
        await navigator.clipboard.writeText(value);
      } catch {
        this.error = what === 'url' ? 'Could not copy the URL.' : 'Could not copy the secret.';
        return;
      }
      this.copied = what;
      window.clearTimeout(this.timer);
      this.timer = window.setTimeout(() => {
        this.copied = '';
      }, COPIED_MS);
    },
  }));
});
