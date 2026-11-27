// Alpine component of the sink of live request card updates (`x-data="requestCards"` on the
// hidden `#request-updates` of the inbox, task and chat pages; `requests.rs`, `sse.rs`).
//
// The server appends each live card as
// `<template data-request-card data-before="<anchor id or empty>" data-into="<list selector>">`
// and each removal as `<template data-request-delete="<card id>">`. This component takes every
// template that arrives:
// - no card with that id on the page: the card goes before the anchor, else at the end of the list;
// - a card with that id: it is replaced; when both the old and the new card are pending and their
//   forms carry the same `data-question`, the old `<form>` moves into the new card first, so
//   checked options, typed text, the open tab and focus stay;
// - a delete template removes the card;
// - a keep template (`data-request-keep="<card ids>"`, the first send of a connection) removes
//   every card of the list it does not name: those were decided while the page was offline.
// Without this script the sink stays hidden and live card updates do nothing until a reload.
document.addEventListener('alpine:init', () => {
  const FORM = 'form[data-question]';

  // The old form of an unchanged pending question replaces the new card's form. Returns a
  // function that gives the focus back to the field that had it, once the card is on the page.
  function keepForm(old: HTMLElement, card: HTMLElement): () => void {
    const none = (): void => {};
    if (old.dataset.state !== 'pending' || card.dataset.state !== 'pending') return none;
    const oldForm = old.querySelector<HTMLElement>(FORM);
    const newForm = card.querySelector<HTMLElement>(FORM);
    if (!oldForm || !newForm || oldForm.dataset.question !== newForm.dataset.question) return none;
    // Moving the form out of the page drops its focus.
    const focused = document.activeElement;
    const field = focused instanceof HTMLElement && oldForm.contains(focused) ? focused : null;
    const text = field instanceof HTMLInputElement || field instanceof HTMLTextAreaElement ? field : null;
    const [start, end] = [text?.selectionStart ?? null, text?.selectionEnd ?? null];
    newForm.replaceWith(oldForm);
    return () => {
      field?.focus();
      if (text && start !== null && end !== null) text.setSelectionRange(start, end);
    };
  }

  // Removes every card of the list that the server no longer names.
  function keepOnly(template: HTMLTemplateElement, ids: string): void {
    const keep = new Set(ids.split(' '));
    const list = document.querySelector(template.dataset.into ?? '#request-list');
    for (const card of list?.querySelectorAll<HTMLElement>(':scope > [data-state]') ?? []) {
      if (!keep.has(card.id)) card.remove();
    }
  }

  function apply(template: HTMLTemplateElement): void {
    const removed = template.dataset.requestDelete;
    if (removed !== undefined) {
      document.getElementById(removed)?.remove();
      return;
    }
    const kept = template.dataset.requestKeep;
    if (kept !== undefined) {
      keepOnly(template, kept);
      return;
    }
    const card = template.content.firstElementChild as HTMLElement | null;
    if (!card) return;
    const old = document.getElementById(card.id);
    if (old) {
      const refocus = keepForm(old, card);
      old.replaceWith(card);
      refocus();
      // htmx does not see nodes the script inserts: without this the form would not post by htmx.
      htmx.process(card);
      return;
    }
    const anchor = template.dataset.before ? document.getElementById(template.dataset.before) : null;
    if (anchor) anchor.before(card);
    else document.querySelector(template.dataset.into ?? '#transcript')?.append(card);
    htmx.process(card);
    // The first card of an empty inbox replaces the empty state.
    document.querySelector('#request-list [data-empty-state]')?.remove();
  }

  Alpine.data('requestCards', () => ({
    observer: null as MutationObserver | null,

    init(): void {
      this.drain();
      this.observer = new MutationObserver(() => this.drain());
      this.observer.observe(this.$el, { childList: true });
    },

    destroy(): void {
      this.observer?.disconnect();
    },

    // Applies and removes every template in the sink.
    drain(): void {
      for (const template of this.$el.querySelectorAll<HTMLTemplateElement>('template')) {
        apply(template);
        template.remove();
      }
    },
  }));
});
