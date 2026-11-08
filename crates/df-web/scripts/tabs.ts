// Alpine component of a form with tabs (`x-data='tabs("general")'`, see `tabbed_form` in
// `config_pages.rs`).
//
// State: `tab`, the `data-tab` of the open tab; the server passes the one to open first.
//
// `adopt()`: sets `role="tabpanel"` and `aria-labelledby` on the panels the tabs control.
//
// `init()`: keeps the open tab in view when the tab row scrolls (phones), on init and on every
// change.
//
// Methods: `reveal()` scrolls the open tab into view. `move(event)` on the tab list: arrow keys go
// to the previous or next tab (wrapping), Home and End to the first or last; the focused tab opens
// (WAI-ARIA tabs pattern, automatic activation). Other keys are left alone.
document.addEventListener('alpine:init', () => {
  Alpine.data('tabs', (initial: string) => ({
    tab: initial,

    init(): void {
      this.$watch('tab', () => this.reveal());
      this.reveal();
    },

    // Makes the panel each tab controls a tabpanel that points back at the tab. A tablist that
    // appears later (the details column gets its tab bar with the first artifact) calls it from
    // `x-init`; panels the server rendered as tabpanels are left as they are.
    adopt(): void {
      for (const tab of this.$el.querySelectorAll<HTMLElement>('[role="tab"]')) {
        const panel = document.getElementById(tab.getAttribute('aria-controls') ?? '');
        if (!panel) continue;
        panel.setAttribute('role', 'tabpanel');
        panel.setAttribute('aria-labelledby', tab.id);
      }
    },

    reveal(): void {
      const tab = [...this.$el.querySelectorAll<HTMLElement>('[role="tab"]')].find((t) => t.dataset.tab === this.tab);
      if (tab) tab.scrollIntoView({ block: 'nearest', inline: 'nearest' });
    },

    move(event: KeyboardEvent): void {
      const list = event.currentTarget as HTMLElement;
      const tabs = [...list.querySelectorAll<HTMLElement>('[role="tab"]')];
      const at = tabs.indexOf(document.activeElement as HTMLElement);
      if (at < 0) return;
      const steps: Record<string, number | undefined> = { ArrowUp: -1, ArrowLeft: -1, ArrowDown: 1, ArrowRight: 1 };
      const step = steps[event.key];
      let next: number;
      if (step !== undefined) next = (at + step + tabs.length) % tabs.length;
      else if (event.key === 'Home') next = 0;
      else if (event.key === 'End') next = tabs.length - 1;
      else return;
      event.preventDefault();
      this.tab = tabs[next].dataset.tab ?? '';
      tabs[next].focus();
    },
  }));
});
