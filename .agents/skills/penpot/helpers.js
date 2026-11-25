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

// Binds a token to each shape one by one: token.applyToShapes([...]) skips cloned shapes silently,
// and applying a token a shape already carries removes the binding, so bound shapes are skipped.
// Returns the names of shapes that still lack the binding (empty when all are bound).
storage.applyTokenEach = async (tokenName, shapes, props = ["fill"]) => {
  const token = penpotUtils.findTokenByName(tokenName);
  if (!token) throw new Error(`token not found: ${tokenName}`);
  const bound = (s) => props.every((p) => s.tokens[p] === tokenName);
  for (const s of shapes.filter((x) => !bound(x))) {
    s.applyToken(token, props);
    await s.waitForLayoutUpdate();
  }
  await new Promise((r) => setTimeout(r, 1000));
  return shapes.filter((s) => !bound(s)).map((s) => s.name);
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

return ["tree", "board", "pageBudget", "overlaps", "applyTokenEach", "centreLabel"].filter((n) => typeof storage[n] === "function");
