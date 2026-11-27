// Alpine component of the body on the workspace pages (`x-data="pageView"`).
//
// On a phone the screen shown (`data-view` of the body) follows the URL: a row click sets it to
// `task`, and history navigation does not undo that. `syncView()` is bound by
// `x-on:popstate.window`: a task or chat URL shows the detail, any other the list.
document.addEventListener('alpine:init', () => {
  Alpine.data('pageView', () => ({
    syncView(): void {
      document.body.dataset.view = /^\/(tasks|chats)\/[A-Z]/.test(location.pathname) ? 'task' : 'list'
    },
  }))
})
