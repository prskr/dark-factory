// The root 404.html is served for every missing path. A path below /docs/<slot>/ gets a link to
// the start page of that version (the docs switcher sends visitors to pages a version lacks).
(() => {
  const slot = /^\/docs\/([A-Za-z0-9.]+)\//.exec(location.pathname)?.[1];
  const link = document.getElementById("df-not-found-docs");
  if (!slot || !link) return;
  link.href = `/docs/${slot}/`;
  link.textContent = `Start page of the ${slot} docs`;
})();
