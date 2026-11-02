// Alpine component of a form with tabs (`x-data='tabs("general")'`, see `tabbed_form` in
// `config_pages.rs`).
//
// State: `tab`, the `data-tab` of the open tab; the server passes the one to open first.
//
// Methods: `move(event)` on the tab list: arrow keys go to the previous or next tab (wrapping),
// Home and End to the first or last; the focused tab opens (WAI-ARIA tabs pattern, automatic
// activation). Other keys are left alone.
document.addEventListener('alpine:init', () => {
  Alpine.data('tabs', (initial) => ({
    tab: initial,

    move(event) {
      const tabs = [...event.currentTarget.querySelectorAll('[role="tab"]')];
      const at = tabs.indexOf(document.activeElement);
      if (at < 0) return;
      const step = { ArrowUp: -1, ArrowLeft: -1, ArrowDown: 1, ArrowRight: 1 }[event.key];
      let next;
      if (step !== undefined) next = (at + step + tabs.length) % tabs.length;
      else if (event.key === 'Home') next = 0;
      else if (event.key === 'End') next = tabs.length - 1;
      else return;
      event.preventDefault();
      this.tab = tabs[next].dataset.tab;
      tabs[next].focus();
    },
  }));
});
