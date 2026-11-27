// Alpine component of the new-task composer (`x-data="composer({ issue, branch })"` on the form).
//
// State: `issue` and `branch` are the linked issue number and the existing branch (the server
// fills both in), `linking` shows the popover with their fields.
//
// The browser remembers the last project and assignee in `localStorage['df.composer']`, read and
// written in `try/catch` (storage may be blocked). `init()` preselects them unless the select has
// `data-explicit` (the project filter chose it) or the remembered value is no longer offered;
// `remember()` is bound to the submit of the form.
interface ComposerArgs {
  issue: string;
  branch: string;
}

const COMPOSER_KEY = 'df.composer'

function preselect(select: HTMLSelectElement, value: unknown): void {
  if (select.dataset.explicit || typeof value !== 'string') return
  if ([...select.options].some((option) => option.value === value)) select.value = value
}

document.addEventListener('alpine:init', () => {
  Alpine.data('composer', (args: ComposerArgs) => ({
    issue: args.issue,
    branch: args.branch,
    linking: false,
    init(): void {
      // The selects register as refs after this component is created.
      this.$nextTick(() => {
        try {
          const saved = JSON.parse(localStorage.getItem(COMPOSER_KEY) ?? '{}')
          preselect(this.$refs.project as HTMLSelectElement, saved.project)
          preselect(this.$refs.persona as HTMLSelectElement, saved.persona)
        } catch (e) {}
      })
    },
    remember(): void {
      const project = (this.$refs.project as HTMLSelectElement).value
      const persona = (this.$refs.persona as HTMLSelectElement).value
      try { localStorage.setItem(COMPOSER_KEY, JSON.stringify({ project, persona })) } catch (e) {}
    },
  }))
})
