// Tests of helpers.js against stubs of the PenPot plugin API: `node --test .agents/skills/penpot/`.
// The stubs model what the helpers rely on: `children` in stacking order (last is in front),
// `findShapes` returns the descendants without the root, and applying a token a shape already
// carries removes the binding (a toggle).
const { test } = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");

const SOURCE = fs.readFileSync(path.join(__dirname, "helpers.js"), "utf8");

const LIGHT = {
  "color.text": "#1C1917",
  "color.ink": "#1C1917",
  "color.rail": "#1C1917",
  "color.app": "#FAFAF9",
  "color.panel": "#FFFFFF",
  "color.accent": "#00FF41",
  "color.accent-soft": "#DCFCE7",
  "color.avatar.1.bg": "#DCFCE7",
  "color.avatar.1.fg": "#166534",
  "color.status.completed.bg": "#DCFCE7",
  "color.status.completed.fg": "#166534",
  "color.status.failed.bg": "#FEE2E2",
  "color.status.waiting.bg": "#FEF3C7",
  "color.border": "#E7E5E4",
};

let ids = 0;

const shape = (props, children = []) => {
  const s = { id: `id-${++ids}`, name: "", type: "board", tokens: {}, fills: [], strokes: [], x: 0, y: 0, width: 10, height: 10, parent: null, ...props, children };
  children.forEach((c) => (c.parent = s));
  s.remove = () => {
    s.parent.children.splice(s.parent.children.indexOf(s), 1);
    s.removed = true;
  };
  s.resize = (w, h) => {
    s.width = w;
    s.height = h;
  };
  s.bringToFront = () => {
    s.parent.children.splice(s.parent.children.indexOf(s), 1);
    s.parent.children.push(s);
  };
  s.appendChild = (c) => {
    c.parent.children.splice(c.parent.children.indexOf(c), 1);
    c.parent = s;
    s.children.push(c);
  };
  s.applyToken = (token, props) => props.forEach((p) => (s.tokens[p] = s.tokens[p] === token.name ? undefined : token.name));
  return s;
};

const fill = (hex, opacity = 1) => [{ fillColor: hex, fillOpacity: opacity }];
const text = (name, characters, hex, props = {}) => shape({ type: "text", name, characters, fills: fill(hex), ...props });

const descendants = (root) => (root.children || []).flatMap((c) => [c, ...descendants(c)]);

// Loads helpers.js into a fresh `storage` against the given pages ({ name: [boards] }).
const load = (pages = { Page: [] }) => {
  const pageList = Object.entries(pages).map(([name, boards]) => ({ id: `page-${name}`, name, root: shape({ name: "Root Frame" }, boards) }));
  const penpot = {
    currentPage: pageList[0],
    openPage: async (id) => (penpot.currentPage = pageList.find((p) => p.id === id)),
    library: { local: { tokens: { sets: [{ name: "light", tokens: Object.entries(LIGHT).map(([name, value]) => ({ name, value, type: "color" })) }], themes: [] } } },
  };
  const penpotUtils = {
    getPages: () => pageList.map(({ id, name }) => ({ id, name })),
    findShapes: (pred, root) => descendants(root).filter(pred),
    findTokenByName: (name) => ({ name }),
  };
  const storage = {};
  const instant = (fn) => fn();
  const installed = new Function("storage", "penpot", "penpotUtils", "setTimeout", SOURCE)(storage, penpot, penpotUtils, instant);
  return { storage, penpot, installed };
};

test("installs every helper it lists", () => {
  const { storage, installed } = load();
  assert.ok(installed.includes("bindPage") && installed.includes("checkPages"));
  installed.forEach((n) => assert.equal(typeof storage[n], "function", n));
});

test("colour rules pick the token of the role", () => {
  const { storage } = load();
  const cases = [
    [text("t", "x", "#1C1917"), "#1C1917", "fill", "color.text"],
    [shape({ name: "Rail" }), "#1C1917", "fill", "color.rail"],
    [shape({ name: "icon" }, [shape({ name: "glyph", strokes: [{ strokeColor: "#00FF41" }] })]), "#1C1917", "fill", "color.rail"],
    [shape({ name: "plain" }), "#1C1917", "fill", "color.ink"],
    [text("t", "x", "#44403C"), "#44403C", "fill", "color.muted"],
    [shape({ name: "translucent" }), "#000000", "fill", null, 0.4],
    [shape({ name: "logo" }), "#D40000", "strokeColor", null],
    [shape({ name: "nothing like it" }), "#123456", "fill", undefined],
  ];
  for (const [s, hex, prop, want, opacity] of cases) assert.equal(storage.colourToken(s, hex, prop, opacity), want, `${s.name} ${hex}`);
});

test("a shared value resolves by role: avatar, status badge, else accent-soft", () => {
  const { storage } = load();
  const avatar = shape({ name: "Avatar" }, [shape({ name: "bg" })]);
  const badge = shape({ name: "Status badge" }, [shape({ name: "bg" })]);
  shape({ name: "Root" }, [avatar, badge]);
  assert.equal(storage.colourToken(avatar.children[0], "#DCFCE7", "fill"), "color.avatar.1.bg");
  assert.equal(storage.colourToken(badge.children[0], "#DCFCE7", "fill"), "color.status.completed.bg");
  assert.equal(storage.colourToken(shape({ name: "chip" }), "#DCFCE7", "fill"), "color.accent-soft");
});

test("text on a status background token outside a badge has no rule, a status foreground has", () => {
  const { storage } = load();
  const row = shape({ name: "row" }, [text("label", "x", "#FEE2E2"), text("Text", "Allow", "#166534")]);
  shape({ name: "Root" }, [row]);
  assert.equal(storage.colourToken(row.children[0], "#FEE2E2", "fill"), undefined);
  assert.equal(storage.colourToken(row.children[1], "#166534", "fill"), "color.status.completed.fg");
});

test("host icon tiles take the tone of their monogram, unknown hosts the neutral tile", () => {
  const { storage } = load();
  const py = shape({ name: "host icon", fills: fill("#1D4ED8") }, [text("Py", "Py", "#FFFFFF")]);
  const other = shape({ name: "host icon", fills: fill("#1D4ED8") }, [text("D", "D", "#FFFFFF")]);
  shape({ name: "Root" }, [py, other]);
  assert.equal(storage.colourToken(py, "#1D4ED8", "fill"), "color.status.waiting.bg");
  assert.equal(storage.colourToken(py.children[0], "#FFFFFF", "fill"), "color.status.waiting.fg");
  assert.equal(storage.colourToken(other, "#1D4ED8", "fill"), "color.sunk");
  assert.equal(storage.colourToken(other.children[0], "#FFFFFF", "fill"), "color.muted");
});

test("unboundColours checks the board itself, skips base-background and samples unknown colours", () => {
  const { storage } = load();
  const board = shape({ name: "Board", fills: fill("#FAFAF9") }, [
    shape({ name: "base-background", type: "rectangle", fills: fill("#FAFAF9") }),
    shape({ name: "card" }, [shape({ name: "odd", fills: fill("#123456") })]),
    shape({ name: "odd 2", fills: fill("#123456") }),
  ]);
  shape({ name: "Root Frame" }, [board]);
  const { jobs, unknown } = storage.unboundColours(board);
  assert.deepEqual(jobs.map((j) => `${j.shape.name}:${j.token}`), ["Board:color.app"]);
  assert.deepEqual(unknown, { "fill #123456": { count: 2, samples: ["Board / card / odd", "Board / odd 2"] } });
});

test("unboundColours reports a shape with several paints, which it cannot bind", () => {
  const { storage } = load();
  const board = shape({ name: "Board" }, [
    shape({ name: "layered", fills: [...fill("#FAFAF9"), ...fill("#1C1917")] }),
    shape({ name: "outlined", strokes: [{ strokeColor: "#E7E5E4" }, { strokeColor: "#1C1917" }] }),
  ]);
  shape({ name: "Root Frame" }, [board]);
  const { jobs, unknown } = storage.unboundColours(board);
  assert.deepEqual(jobs, []);
  assert.deepEqual(unknown, {
    "fill: 2 paints": { count: 1, samples: ["Board / layered"] },
    "strokeColor: 2 paints": { count: 1, samples: ["Board / outlined"] },
  });
});

test("bindColours binds once, removes base-background and leaves nothing unbound", async () => {
  const { storage } = load();
  const bg = shape({ name: "base-background", type: "rectangle", fills: fill("#FAFAF9") });
  const board = shape({ name: "Board", fills: fill("#FAFAF9") }, [bg, text("t", "x", "#1C1917")]);
  shape({ name: "Root Frame" }, [board]);
  const r = await storage.bindColours(board);
  assert.deepEqual([r.bound, r.left, bg.removed], [2, [], true]);
  assert.equal(board.children[0].tokens.fill, "color.text");
});

test("trimToOverlay keeps the named shapes, nested ones too, stacked in keep order", () => {
  const menu = shape({ name: "Menu", x: 300, y: 100, width: 100, height: 50 });
  const panel = shape({ name: "Events panel", x: 200, y: 60, width: 400, height: 300 });
  const main = shape({ name: "Main", x: 100, y: 0, width: 600, height: 400 }, [shape({ name: "Graph panel" }), panel]);
  const board = shape({ name: "Screen", width: 700, height: 400, flex: { remove() { board.flex = null; } } }, [menu, shape({ name: "Rail" }), main]);
  const { storage } = load({ Page: [board] });
  storage.trimToOverlay("Screen", ["Events panel", "Menu"], 10);
  assert.deepEqual(board.children.map((c) => c.name), ["Events panel", "Menu"]);
  assert.deepEqual([board.flex, board.width, board.height], [null, 420, 320]);
  assert.deepEqual([panel.x, panel.y, menu.x, menu.y], [10, 10, 110, 50]);
});

test("trimToOverlay throws on a missing name and leaves the board as it was", () => {
  const board = shape({ name: "Screen" }, [shape({ name: "Rail" }), shape({ name: "Main" })]);
  const { storage } = load({ Page: [board] });
  assert.throws(() => storage.trimToOverlay("Screen", ["Main", "Modal"]), /lacks/);
  assert.deepEqual(board.children.map((c) => c.name), ["Rail", "Main"]);
});

test("trimToOverlay throws on an empty keep list and leaves the board as it was", () => {
  const board = shape({ name: "Screen" }, [shape({ name: "Rail" }), shape({ name: "Main" })]);
  const { storage } = load({ Page: [board] });
  assert.throws(() => storage.trimToOverlay("Screen", []), /keeps nothing/);
  assert.deepEqual([board.children.map((c) => c.name), board.width], [["Rail", "Main"], 10]);
});

test("bindPage binds every board of a page and reports what is left", async () => {
  const a = shape({ name: "A", fills: fill("#FAFAF9") }, [text("t", "x", "#1C1917")]);
  const b = shape({ name: "B" }, [shape({ name: "odd", fills: fill("#123456") })]);
  const { storage, penpot } = load({ Other: [], Target: [a, b] });
  const r = await storage.bindPage("Target");
  assert.equal(penpot.currentPage.name, "Target");
  assert.deepEqual([r.page, r.applied, r.left, r.suspect], ["Target", 2, [], []]);
  assert.deepEqual(Object.keys(r.unknown), ["fill #123456"]);
});

test("checkPages reports missing, extra and duplicate boards and overlaps", async () => {
  const at = (name, x) => shape({ name, x, width: 100, height: 100 });
  const { storage } = load({ One: [at("A", 0), at("A", 0), at("Stray", 500)], Two: [at("C", 0)] });
  const r = await storage.checkPages({ One: ["A", "B"], Two: ["C"] });
  assert.deepEqual(r.One, { missing: ["B"], extra: ["Stray"], dupes: ["A"], overlaps: ["A <> A"], shapes: 3 });
  assert.deepEqual(r.Two, { missing: [], extra: [], dupes: [], overlaps: [], shapes: 1 });
});
