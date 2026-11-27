// Alpine component of the options of a question card (`x-data='questionOther("<value>")'` on the
// fieldset, see `choice_input` in `requests/question.rs`; the argument is the form value of the
// Other answer, `OTHER_VALUE` of `df-core`).
//
// The input of the Other answer shows while Other is checked and takes focus when it gets
// checked; unchecking hides it. A change inside the fieldset runs `sync(true)` (a radio that
// replaces Other fires `change` on the new radio). Without this script the input is always
// shown, and its text counts only when Other is checked (the server ignores it otherwise).
document.addEventListener('alpine:init', () => {
  Alpine.data('questionOther', (otherValue: string) => ({
    open: false,

    init(): void {
      this.sync(false);
    },

    // Shows the Other input when the Other answer is checked; `focus` moves the caret into it
    // when Other just got checked (the input's own `change` on blur must not pull focus back).
    sync(focus: boolean): void {
      const other = this.$el.querySelector<HTMLInputElement>(`input[value="${otherValue}"]`);
      const wasOpen = this.open;
      this.open = Boolean(other?.checked);
      if (wasOpen || !this.open || !focus) return;
      void this.$nextTick(() => this.$el.querySelector<HTMLInputElement>('[data-other-input]')?.focus());
    },
  }));
});
