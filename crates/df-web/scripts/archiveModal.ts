// Alpine component of the archive modal of the chat page (`x-data="archiveModal({...})"` on
// `#archive-body`, see `archive.rs`). The server renders every state of the modal (list, search,
// selected chat, tab, opened artifact); this component only keeps which screen a phone shows.
//
// Config: `selected` (a chat is selected).
//
// State: `chat`, the chat pane is the screen (below 768 px; from there both panes show). Selecting a
// chat sets it, the back action of the pane clears it. A search or a tab swaps the body, which starts
// the component again with the screen the server rendered for.
interface ArchiveModalConfig {
  selected: boolean;
}

document.addEventListener('alpine:init', () => {
  Alpine.data('archiveModal', (cfg: ArchiveModalConfig) => ({
    chat: cfg.selected,
  }));
});
