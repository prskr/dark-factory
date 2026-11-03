// Alpine component of the "New egress rule" dialog (`x-data='egressRule({"effect": "allow"})'`,
// see `host_form` in `src/egress/allowlist.rs`).
//
// State: `effect`, `allow` or `deny`; `deny` is true for a deny rule.
//
// A deny rule applies to every project and port, so "Applies to" and Port are disabled and show
// `every project` and `any`. Switching back restores what was typed. Disabled fields are not
// submitted, and the server ignores project and port for a deny rule anyway.
document.addEventListener('alpine:init', () => {
  Alpine.data('egressRule', (cfg) => ({
    effect: cfg.effect,
    typed: {},

    get deny() {
      return this.effect === 'deny';
    },

    init() {
      this.$watch('effect', () => {
        for (const [ref, shown] of [['project', ''], ['port', 'any']]) {
          const el = this.$refs[ref];
          if (this.deny) {
            this.typed[ref] = el.value;
            el.value = shown;
          } else if (ref in this.typed) {
            el.value = this.typed[ref];
          } else if (ref === 'port') {
            el.value = '';
          }
        }
      });
    },
  }));
});
