// Dark Factory PenPot helpers. Pass this file's content unchanged as the `code` of one
// `execute_code` call: at the start of a session, and again whenever `storage.tree` is missing
// (the plugin's `storage` is empty after a reconnect, a tab reload or another browser).

// Compact structure of a shape: name, type, flex direction, size, text and the id's last 6 chars.
// Much smaller than penpotUtils.shapeStructure, which overflows the tool output on full boards.
storage.tree = (shape, maxDepth = 4) => {
  const lines = [];
  const walk = (s, depth) => {
    const flex = s.flex ? ` flex-${s.flex.dir}` : "";
    const text = s.type === "text" ? ` "${s.characters.slice(0, 40)}" ${s.fontSize}/${s.fontWeight}` : "";
    lines.push(`${"  ".repeat(depth)}${s.name} [${s.type}${flex}] ${Math.round(s.width)}x${Math.round(s.height)}${text} #${s.id.slice(-6)}`);
    if (depth < maxDepth && s.children) s.children.forEach((c) => walk(c, depth + 1));
  };
  walk(shape, 0);
  return lines.join("\n");
};

// A top-level board of the current page by name (ids from earlier calls are gone after a reconnect).
storage.board = (name) => penpot.currentPage.root.children.find((c) => c.name === name);

// Shape count of the current page, for the page budget of about 1,000 shapes.
storage.pageBudget = () => {
  const root = penpot.currentPage.root;
  return { page: penpot.currentPage.name, boards: root.children.length, shapes: penpotUtils.findShapes(() => true, root).length };
};

// Pairs of top-level boards on the current page that overlap.
storage.overlaps = () => {
  const boards = penpot.currentPage.root.children.filter((c) => c.type === "board");
  const hit = (a, b) => a.x < b.x + b.width && b.x < a.x + a.width && a.y < b.y + b.height && b.y < a.y + a.height;
  return boards.flatMap((a, i) => boards.slice(i + 1).filter((b) => hit(a, b)).map((b) => `${a.name} <> ${b.name}`));
};

// Switches the theme of group `Mode` ("Light" or "Dark") and returns the state of both themes.
storage.setMode = async (name) => {
  const modes = penpot.library.local.tokens.themes.filter((t) => t.group === "Mode");
  const theme = modes.find((t) => t.name === name);
  if (!theme) throw new Error(`theme not found: ${name}`);
  if (!theme.active) theme.toggleActive();
  await new Promise((r) => setTimeout(r, 1500));
  return modes.map((t) => `${t.name}:${t.active}`);
};

// Binds a token to each shape. token.applyToShapes([...]) skips cloned shapes silently, and applying
// a token a shape already carries removes the binding (a toggle), so bound shapes are skipped.
// No waitForLayoutUpdate per shape: it can stall for minutes; one pause lets PenPot apply all.
// Returns the names of shapes that still lack the binding (empty when all are bound).
storage.applyTokenEach = async (tokenName, shapes, props = ["fill"]) => {
  const token = penpotUtils.findTokenByName(tokenName);
  if (!token) throw new Error(`token not found: ${tokenName}`);
  const bound = (s) => props.every((p) => s.tokens[p] === tokenName);
  shapes.filter((s) => !bound(s)).forEach((s) => s.applyToken(token, props));
  await new Promise((r) => setTimeout(r, 2500));
  return shapes.filter((s) => !bound(s)).map((s) => s.name);
};

// Off-token colours of older boards, mapped to the token the web UI uses for the same element
// (`crates/df-web/styles/input.css`, `crates/df-web/src`).
const OFF_TOKEN = {
  "#78716C": "color.faint", // icon strokes
  "#8A8A85": "color.faint",
  "#D97706": "color.status.limited.fg", // needs-you dot: `needs_you_dot` uses Tone::Limited
  "#D6D3D1": "color.border-strong", // sheet grabber
  "#15803D": "color.avatar.1.fg", // avatar initials
  "#0369A1": "color.status.running.fg", // icon on the image tile (`color.status.running.bg`)
  "#C9C9C3": "color.border-strong", // scrollbar thumb
  "#16A34A": "color.status.completed.fg", // live and done dots: `bg-status-completed-fg`
  "#14532D": "color.status.completed.fg", // verdict pill and Live badge: `tone_badge` completed
  "#F0EFEE": "color.panel", // verdict filter: `bg-panel`
  "#DC2626": "color.failed", // blocked graph edges and arrows: `stroke-danger`, `fill-danger`
};

// Host icon tiles take the status tone of their provider (`brands.json`), keyed by the monogram
// on the tile. A host that is no provider gets the neutral tile, like the globe (`host_icon.rs`).
const HOST_TONE = { A: "limited", GH: "queued", n: "failed", Go: "running", Py: "waiting", G: "completed" };

const hostTileToken = (shape, fg) => {
  let tile = shape;
  while (tile && !/host icon/i.test(tile.name)) tile = tile.parent;
  if (!tile) return undefined;
  const monogram = penpotUtils.findShapes((s) => s.type === "text", tile).map((s) => s.characters).join("");
  const tone = HOST_TONE[monogram];
  const own = shape.id === tile.id && !fg;
  if (!tone) return own ? "color.sunk" : "color.muted";
  return `color.status.${tone}.${own ? "bg" : "fg"}`;
};

// Colours that stay raw: brand colours the web UI fixes (Forgejo logo in `icons.rs`), translucent
// borders drawn as their mixed value (`border-danger/40`, `border-status-completed-fg/40`), and
// what is not UI (PenPot's variant container outline, sticky notes).
const RAW = new Set(["#D40000", "#FCA5A5", "#86EFAC", "#BB97D8", "#FDBA74"]);

const SUSPECT_TEXT = /^color\.(rail|border|status\.[a-z]+\.bg)/;

// A dark tile with an accent glyph (sandbox icon, logo) is `bg-rail text-accent` in the web UI.
const holdsAccent = (shape) =>
  penpotUtils.findShapes(
    (s) => s.tokens?.fill === "color.accent" || s.tokens?.strokeColor === "color.accent" || [s.fills?.[0]?.fillColor, s.strokes?.[0]?.strokeColor].some((c) => c?.toUpperCase() === "#00FF41"),
    shape,
  ).length > 0;

const hasAncestor = (shape, pattern) => {
  for (let p = shape.parent; p && p.parent; p = p.parent) {
    if (pattern.test(p.name) || pattern.test(p.tokens?.fill || "")) return true;
  }
  return false;
};

// The token for a raw colour, decided by value and role. Returns null for colours that stay raw
// (translucent: scrims, like the web UI's fixed `bg-black/40`) and undefined for a colour without
// a rule: decide it, add it to OFF_TOKEN or a role rule here, reinstall.
storage.colourToken = (shape, hex, prop, opacity) => {
  if (opacity != null && opacity < 1) return null;
  hex = hex.toUpperCase();
  if (RAW.has(hex)) return null;
  if (/host icon/i.test(shape.name) || hasAncestor(shape, /host icon/i)) return hostTileToken(shape, shape.type === "text" || prop === "strokeColor");
  const fg = shape.type === "text" || prop === "strokeColor";
  const inAgent = /agent|bubble/i.test(shape.name) || hasAncestor(shape, /agent|bubble/i);
  // Persona avatars use the palette `color.avatar.N`; the user's own avatar (account, profile menu)
  // is `color.accent-soft` like the web UI's `avatar()`.
  const inAvatar = !hasAncestor(shape, /^(account|profile)/i) && (/avatar/i.test(shape.name) || hasAncestor(shape, /^avatar/i));
  const inStatus = /status|badge/i.test(shape.name) || hasAncestor(shape, /status|badge/i);
  // Values that several tokens share, or that mean another role on text.
  if (hex === "#1C1917") return fg ? "color.text" : /rail/i.test(shape.name) || holdsAccent(shape) ? "color.rail" : inAgent ? "color.agent-bg" : "color.ink";
  if (hex === "#FAFAF9") return fg ? (inAgent ? "color.agent-fg" : "color.on-ink") : "color.app";
  if (hex === "#F5F5F4") return "color.sunk";
  if (hex === "#E7E5E4") return inAvatar ? "color.avatar-me" : "color.border";
  if (shape.type === "text" && hex === "#292524") return "color.text"; // prose, not the rail
  if (shape.type === "text" && hex === "#A8A29E") return "color.faint"; // placeholders, keys
  if (shape.type === "text" && hex === "#44403C") return inStatus ? "color.status.queued.fg" : "color.muted"; // labels: `text-muted`
  const candidates = penpot.library.local.tokens.sets
    .find((s) => s.name === "light")
    .tokens.filter((t) => t.type === "color" && t.value.toUpperCase() === hex)
    .map((t) => t.name);
  const byRole = (inAvatar && candidates.find((n) => n.startsWith("color.avatar."))) || (inStatus && candidates.find((n) => n.startsWith("color.status.")));
  if (byRole) return byRole;
  const token =
    OFF_TOKEN[hex] ??
    (candidates.length === 1 ? candidates[0] : ["color.accent-soft", "color.status."].map((p) => candidates.find((n) => n.startsWith(p))).find(Boolean) ?? candidates[0]);
  // Text on a rail, border or status background token outside a badge is a role the value can't
  // tell. Status foregrounds are text colours (counts, segments, notices).
  if (shape.type === "text" && SUSPECT_TEXT.test(token || "") && !/status|badge|chip/i.test(`${shape.name} ${shape.parent?.name}`)) return undefined;
  return token;
};

// The names from `board` down to `shape`, for finding an unknown colour on the board.
const pathOf = (shape, board) => {
  const names = [];
  for (let s = shape; s && s.id !== board.id; s = s.parent) names.unshift(s.name);
  return [board.name, ...names].join(" / ");
};

// Unbound fills and strokes of a board (the board itself included), skipping `base-background`.
// Returns { jobs: [{ shape, token, prop }], unknown: { "fill #HEX": { count, samples } } } with up
// to three sample paths per unknown colour. A shape with several fills or strokes can't take one
// token; it is reported as "fill: N paints" for a decision by hand.
storage.unboundColours = (board) => {
  const jobs = [];
  const unknown = {};
  const report = (key, shape) => {
    const entry = (unknown[key] ??= { count: 0, samples: [] });
    entry.count++;
    if (entry.samples.length < 3) entry.samples.push(pathOf(shape, board));
  };
  for (const shape of [board, ...penpotUtils.findShapes(() => true, board)]) {
    if (shape.name === "base-background") continue;
    const paints = [
      ["fill", Array.isArray(shape.fills) ? shape.fills : [], "fillColor", "fillOpacity"],
      ["strokeColor", Array.isArray(shape.strokes) ? shape.strokes : [], "strokeColor", "strokeOpacity"],
    ];
    for (const [prop, list, colour, alpha] of paints) {
      if (shape.tokens[prop] || !list.some((p) => p[colour])) continue;
      if (list.length > 1) {
        report(`${prop}: ${list.length} paints`, shape);
        continue;
      }
      const token = storage.colourToken(shape, list[0][colour], prop, list[0][alpha]);
      if (token) jobs.push({ shape, token, prop });
      else if (token === undefined) report(`${prop} ${list[0][colour]}`, shape);
    }
  }
  return { jobs, unknown };
};

// Binds every unbound colour of a board to its token and deletes the empty icon backgrounds.
// Run it once per board and call; on a timeout, query storage.unboundColours before running it again.
storage.bindColours = async (board) => {
  penpotUtils.findShapes((s) => s.name === "base-background" && s.type === "rectangle", board).forEach((s) => s.remove());
  const { jobs, unknown } = storage.unboundColours(board);
  const tokens = {};
  jobs.forEach((j) => j.shape.applyToken((tokens[j.token] ??= penpotUtils.findTokenByName(j.token)), [j.prop]));
  await new Promise((r) => setTimeout(r, 2500));
  const left = storage.unboundColours(board).jobs.map((j) => `${j.shape.name}:${j.prop}`);
  return { board: board.name, bound: jobs.length - left.length, left, unknown };
};

const openPage = async (name) => {
  const page = penpotUtils.getPages().find((p) => p.name === name);
  if (!page) throw new Error(`page not found: ${name}`);
  await penpot.openPage(page.id);
  await new Promise((r) => setTimeout(r, 500));
};

const mergeUnknown = (into, from) => {
  for (const [key, { count, samples }] of Object.entries(from)) {
    const entry = (into[key] ??= { count: 0, samples: [] });
    entry.count += count;
    entry.samples.push(...samples.slice(0, 3 - entry.samples.length));
  }
  return into;
};

// Binds every board of a page: one page per call. Waits once before returning, so the bindings
// land before the next call switches the page (a page switch drops bindings still in flight).
storage.bindPage = async (name) => {
  await openPage(name);
  const boards = penpot.currentPage.root.children.filter((c) => c.type === "board");
  const tokens = {};
  const unknown = {};
  let applied = 0;
  for (const board of boards) {
    penpotUtils.findShapes((s) => s.name === "base-background" && s.type === "rectangle", board).forEach((s) => s.remove());
    const found = storage.unboundColours(board);
    mergeUnknown(unknown, found.unknown);
    found.jobs.forEach((j) => j.shape.applyToken((tokens[j.token] ??= penpotUtils.findTokenByName(j.token)), [j.prop]));
    applied += found.jobs.length;
  }
  await new Promise((r) => setTimeout(r, 3000));
  const left = boards.flatMap((b) => storage.unboundColours(b).jobs.map((j) => `${b.name} / ${j.shape.name}:${j.prop}`));
  return { page: name, applied, left, unknown, suspect: storage.suspectBindings() };
};

// Checks pages against the boards they should hold ({ page: [board names] }): missing, extra and
// duplicate boards, overlaps and the shape count. Read-only; about ten pages fit in one call.
storage.checkPages = async (expected) => {
  const result = {};
  for (const [name, boards] of Object.entries(expected)) {
    await openPage(name);
    const have = penpot.currentPage.root.children.map((c) => c.name);
    result[name] = {
      missing: boards.filter((n) => !have.includes(n)),
      extra: have.filter((n) => !boards.includes(n)),
      dupes: have.filter((n, i) => have.indexOf(n) !== i),
      overlaps: storage.overlaps(),
      shapes: storage.pageBudget().shapes,
    };
  }
  return result;
};

// Text bound to a rail, border or status background token outside a badge: likely a wrong role.
storage.suspectBindings = (root = penpot.currentPage.root) =>
  penpotUtils
    .findShapes((s) => s.type === "text" && SUSPECT_TEXT.test(s.tokens?.fill || ""), root)
    .filter((s) => !/status|badge|chip/i.test(`${s.name} ${s.parent?.name}`))
    .map((s) => `${s.tokens.fill}: ${s.parent?.name} / ${s.characters.slice(0, 30)}`);

// Turns a full-screen board into a partial board: keeps the named children (a dialog, a menu),
// removes the rest and the flex layout, and shrinks the board around them with `pad` px.
// Without the flex layout the kept children stack in reverse, so they are restacked in the order
// of `keep`: list the base first and the overlay (menu, popover, tooltip) last.
// A name that is no direct child is looked up among the descendants (a panel inside `Main`) and
// moved up to the board.
storage.trimToOverlay = (name, keep, pad = 24) => {
  const board = storage.board(name);
  if (keep.length === 0) throw new Error(`${name}: keep is empty, the cut keeps nothing`);
  const kept = keep.map((n) => board.children.find((c) => c.name === n) ?? penpotUtils.findShapes((s) => s.name === n, board)[0]);
  if (kept.some((c) => !c)) throw new Error(`${name} lacks one of: ${keep.join(", ")}`);
  const rel = kept.map((c) => ({ c, x: c.x - board.x, y: c.y - board.y }));
  const minX = Math.min(...rel.map((r) => r.x));
  const minY = Math.min(...rel.map((r) => r.y));
  const maxX = Math.max(...rel.map((r) => r.x + r.c.width));
  const maxY = Math.max(...rel.map((r) => r.y + r.c.height));
  if (board.flex) board.flex.remove();
  rel.filter((r) => r.c.parent.id !== board.id).forEach((r) => board.appendChild(r.c));
  board.children.filter((c) => !kept.some((k) => k.id === c.id)).forEach((c) => c.remove());
  kept.forEach((c) => c.bringToFront());
  board.resize(maxX - minX + 2 * pad, maxY - minY + 2 * pad);
  rel.forEach((r) => {
    r.c.x = board.x + pad + r.x - minX;
    r.c.y = board.y + pad + r.y - minY;
  });
  return { id: board.id, width: board.width, height: board.height };
};

// Centres the text child of a shape (avatar initials, badge labels): an auto-width text box
// centred by flex leaves the glyphs off-centre. Keeps the text's weight and its fill or fill token.
storage.centreLabel = async (shape) => {
  const text = shape.children.find((c) => c.type === "text");
  if (!text) throw new Error(`${shape.name} has no text child`);
  const { fontWeight, fills } = text;
  const fillToken = text.tokens.fill;
  if (text.layoutChild) {
    text.layoutChild.horizontalSizing = "fix";
    text.layoutChild.verticalSizing = "fix";
  }
  text.growType = "fixed";
  text.resize(shape.width, shape.height);
  text.align = "center";
  text.verticalAlign = "center";
  text.lineHeight = "1";
  text.fontWeight = fontWeight;
  if (fillToken) return storage.applyTokenEach(fillToken, [text]);
  text.fills = fills;
  return [];
};

return [
  "tree", "board", "pageBudget", "overlaps", "setMode", "applyTokenEach", "colourToken",
  "unboundColours", "bindColours", "bindPage", "checkPages", "suspectBindings", "trimToOverlay", "centreLabel",
].filter((n) => typeof storage[n] === "function");
