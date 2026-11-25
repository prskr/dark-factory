// Alpine component of the group "Commits" of the persona form (`x-data="copyValues()"`, see
// `config_pages/signing_key.rs`): copy buttons for the commit email, the public signing key and its
// fingerprint.
//
// State: `copied` (the name of the value copied in the last 2 s), `error` (shown below the fields),
// `canCopy` (the browser offers the asynchronous clipboard API, which needs a secure context: HTTPS
// or a loopback host; the buttons are hidden without it).
//
// `copy(what, value)`: copies `value` and remembers `what` for the "Copied" status.
// Classic scripts share one global scope: the name must differ from the ones of the other components.
const COPY_STATUS_MS = 2000;

document.addEventListener('alpine:init', () => {
  Alpine.data('copyValues', () => ({
    copied: '',
    error: '',
    canCopy: window.isSecureContext && Boolean(navigator.clipboard),
    timer: 0,

    async copy(what: string, value: string): Promise<void> {
      this.error = '';
      try {
        await navigator.clipboard.writeText(value);
      } catch {
        this.error = 'Could not copy the value.';
        return;
      }
      this.copied = what;
      window.clearTimeout(this.timer);
      this.timer = window.setTimeout(() => {
        this.copied = '';
      }, COPY_STATUS_MS);
    },
  }));
});
