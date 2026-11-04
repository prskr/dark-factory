// Version switcher and banner of the docs (docs/layouts/_partials/custom/head-end.html loads it).
// The build knows its own version and slot (<meta name="df-docs-*">); the list of releases is
// fetched from /versions.json at page load, so frozen old builds see releases published later.
(() => {
  const meta = (name) => document.querySelector(`meta[name="df-docs-${name}"]`)?.content;
  const version = meta("version") || "dev";
  const slot = meta("slot") || "dev";
  // /docs/<slot>/<rest>; anything else (hugo server) leaves the page where it is.
  const [, rest] = /^\/docs\/[^/]+\/(.*)$/.exec(location.pathname) ?? [];
  const inSite = rest !== undefined;

  const fetchVersions = async () => {
    try {
      const response = await fetch("/versions.json", { cache: "no-cache" });
      if (!response.ok) return [];
      return (await response.json()).versions ?? [];
    } catch {
      return [];
    }
  };

  const target = (name) => `/docs/${encodeURIComponent(name)}/${rest}${location.search}${location.hash}`;

  // The Versions menu entry (config/_default/menus.yaml) has one placeholder child, rendered by
  // Hextra as a dropdown on wide screens and as a group in the mobile menu. Each placeholder is
  // replaced by one link per version, pointing at the same page in that version.
  const fillVersionMenus = (releases) => {
    // versions.json lists `dev` first once a release exists; it is implicit before that.
    const names = [...new Set(["dev", ...releases.map((r) => r.version)])];
    // /docs/latest/ shows the release the build was made for.
    const current = slot === "latest" ? version : slot;
    for (const placeholder of document.querySelectorAll('a[href$="#df-version"]')) {
      const item = placeholder.closest("li");
      if (!item) continue;
      for (const name of names) {
        const entry = item.cloneNode(true);
        const link = entry.querySelector("a");
        link.href = inSite ? target(name) : `/docs/${encodeURIComponent(name)}/`;
        link.textContent = releases.find((r) => r.version === name)?.latest ? `${name} (latest)` : name;
        if (name === current) {
          link.setAttribute("aria-current", "page");
          link.classList.add("df-version-current");
        }
        item.before(entry);
      }
      item.remove();
    }
    // The dropdown button names the version shown.
    for (const toggle of document.querySelectorAll('.hextra-nav-menu-toggle[aria-label="Versions"]')) {
      const label = toggle.querySelector("span");
      if (label) label.textContent = current;
    }
  };

  // Home and Features (config/_default/menus.yaml) are absolute URLs of the landing page, which
  // Hextra opens in a new tab; they belong to the same site, so they open in place.
  const landingLinksInPlace = () => {
    for (const link of document.querySelectorAll('a[href^="https://www.dark-factory.eu/"]')) {
      const url = new URL(link.href);
      if (url.pathname.startsWith("/docs/")) continue;
      link.href = `${url.pathname}${url.hash}`;
      link.removeAttribute("target");
      link.removeAttribute("rel");
    }
  };

  const banner = (text, href, linkText) => {
    const el = document.createElement("div");
    el.className = "df-docs-banner";
    el.setAttribute("role", "note");
    el.append(text);
    if (href) {
      const a = document.createElement("a");
      a.href = href;
      a.textContent = linkText;
      el.append(" ", a);
    }
    document.body.prepend(el);
  };

  const render = (releases) => {
    fillVersionMenus(releases);
    const newest = releases.find((r) => r.latest)?.version;
    if (version === "dev") {
      banner("Unreleased: these docs describe the main branch, not a release.");
    } else if (newest && newest !== version) {
      banner(`Docs for ${version}. The latest release is ${newest}.`, inSite ? `/docs/latest/${rest}` : "/docs/latest/", "Read the latest docs");
    }
  };

  landingLinksInPlace();
  fetchVersions().then(render);
})();
