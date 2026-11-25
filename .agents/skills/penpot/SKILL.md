---
name: penpot
description: Drawing and changing boards in the PenPot file "App Design" through the PenPot MCP. Use before the first `execute_code` call of a session, when creating or changing boards, binding colours to design tokens, cleaning up or splitting a page, exporting a board to check it, or when PenPot calls time out, writes fail or the plugin reconnects.
---

# PenPot boards

`docs/design-workflow.md` owns the setup check ("Check the setup"), the page table ("The design file") and the board mapping. This skill covers working inside the file.

## Helpers

[`helpers.js`](helpers.js) installs tested helpers into the plugin's `storage`. Pass the file's content unchanged as the `code` of one `execute_code` call at the start of a session; it returns the names it installed. The plugin empties `storage` on every reconnect, tab reload or browser switch: when `storage.tree` is missing, install them again before anything else.

| Helper | Use |
|---|---|
| `storage.tree(shape, maxDepth = 4)` | Compact structure (name, type, size, text, id suffix), instead of `penpotUtils.shapeStructure`, which overflows the output |
| `storage.board(name)` | A top-level board of the current page by name |
| `storage.pageBudget()` | Board and shape count of the current page |
| `storage.overlaps()` | Overlapping top-level boards of the current page |
| `await storage.setMode(name)` | Switches theme group `Mode` to `Light` or `Dark` |
| `await storage.bindColours(board)` | Binds every unbound fill and stroke of a board (itself included) to its token and deletes `base-background` rectangles; returns the count, what is left and the colours without a rule |
| `storage.unboundColours(board)` | Dry run of `bindColours`: `{ jobs, unknown }`; `unknown` holds per colour its count and up to three sample paths (`Board / card / shape`); shapes with several fills or strokes show as `fill: N paints` and need a decision by hand |
| `await storage.bindPage(name)` | Opens the page and binds every board of it, waiting before it returns; returns the count, what is left, unknown colours and suspect bindings |
| `await storage.checkPages({ page: [boards] })` | Per page: missing, extra and duplicate boards, overlaps, shape count; read-only |
| `storage.colourToken(shape, hex, prop, opacity)` | The colour rules: value plus role (text, rail, agent bubble, persona or own avatar, status badge, host icon tile by monogram) to token; `null` keeps a colour raw (translucent, Forgejo logo, not UI), `undefined` means no rule |
| `storage.suspectBindings()` | Texts on a rail, border or status background token outside a badge: likely a wrong role |
| `await storage.applyTokenEach(tokenName, shapes, props = ["fill"])` | Binds one token to the given shapes, skipping bound ones; returns the names still unbound |
| `storage.trimToOverlay(name, keep, pad = 24)` | Turns a full-screen board into a partial board of the named shapes (dialog, menu, a column, a panel nested in `Main`); list the base first and the overlay last, the order is the stacking; throws if a name is missing |
| `await storage.centreLabel(shape)` | Centres the text child of an avatar or badge, keeping weight and fill token |
| `storage.text(characters, { size, weight, mono, colour, name })` | A new text with family, weight variant and size set on its whole range (Inter, or JetBrains Mono with `mono`); defaults 13 / 400 / `#1C1917` |
| `await storage.comments()` | Open comment threads of the current page: `{ seq, board, comments: ["author: text"] }` |

## Before the first write

1. Run "Check the setup" from `docs/design-workflow.md`: `penpot.currentFile.name` is `App Design` and `penpotUtils.getPages()` lists the pages of the page table. Install the helpers.
2. Open the page you will write to (`await penpot.openPage(page)`) and measure it with `storage.pageBudget()`. Keep the **page budget**: about 1,000 shapes per page. A heavy page makes the browser tab freeze, and calls then time out or writes fail half-way. One page with ~9,500 shapes froze on every export. A cloned 1440 px board costs 300–600 shapes.
   - If the new boards would push the page past the budget, don't add to the full page. The plugin API can neither clone nor move a shape to another page, so ask the user to create a new page `<Section> - <Topic>` and paste a copy of the nearest board onto it (copy it in the PenPot UI on the full page, paste it on the new page). Add the page to the page table in `docs/design-workflow.md`, then draw on the new page, cloning from that copy.
3. Plan the fewest boards that show every state the spec names. One board per visually different state. Show states that differ only in one control inside one board, or as a small partial board (the part that changes, not the whole screen; `storage.trimToOverlay` cuts a dialog or modal board down to it).

## Drawing a board

1. Clone the nearest existing board (`board.clone()`), name it `<View> · <state>`, and place it in free space next to the board it derives from.
2. Change the clone; don't redraw what exists. Detach a component copy (`shape.detach()`) only where you change its structure.
3. Bind every colour to a token of the sets `light` and `dark`: see "Binding colours".
4. Work in small calls: one board, or one part of a board, per `execute_code`.
5. Export each board with `export_shape` (by shape id) and look at it. For the dark preview, `storage.setMode("Dark")`, export, and `storage.setMode("Light")`.
6. Check every touched page with `storage.overlaps()`.
7. Read `await storage.comments()` on every touched page and work each thread in. Before removing or re-cutting a board, read its threads first: a thread on a removed board is gone.

Done when every new board is exported in light and dark, no new board overlaps another, the theme is back on `Light`, every touched page is within the page budget, and every open comment thread is worked in or answered.

## Binding colours

The web UI is the reference for every colour role. Where the roles live in `crates/df-web`:

| Element | Source |
|---|---|
| Tokens, light and dark values | `styles/input.css` |
| Badges, status dots, verdict pills, host icon tiles (tone per provider in `components/brands.json`) | `Tone` in `src/components/badges.rs`, `src/components/host_icon.rs` |
| Buttons (primary, secondary, ghost, danger), icon buttons | `src/components/actions.rs` |
| Avatars | `src/components/persona_avatar.rs`, `avatar()` in `src/components/shell.rs` |
| Events table rows, graph edges and nodes | `src/egress/log/rows.rs`, `src/egress/log/graph.rs` |

Token names that differ between `input.css` and PenPot:

| `input.css` / Tailwind | PenPot token |
|---|---|
| `accent-text` (links, "Copied", success status) | `color.accent-strong` |
| `danger` (`text-danger`, error lines) | `color.failed` |

When you draw a new element, take its raw colour from the light value of its PenPot token (`penpot.library.local.tokens.sets`), so the rules bind it.

1. Theme on `Light`: the rules compare raw colours with the light values, and under `Dark` every bound fill reports its dark value. Run `storage.unboundColours(board)` and read `unknown`; its sample paths name the shapes.
2. For each unknown colour, find the element in the web UI and the token it uses. A colour the UI draws with an existing token goes into `OFF_TOKEN` in `helpers.js`; a value whose token depends on the element gets a role rule in `storage.colourToken`. A colour the UI lacks becomes a new token in both sets. Add a case to `helpers.test.js` (`node --test .agents/skills/penpot/helpers.test.js`; pre-commit runs it), then reinstall the helpers by passing the whole file: a partial reinstall drifts from it.
3. `await storage.bindColours(board)` for one board, `await storage.bindPage(name)` for a whole page; one page per call. Done when `left` and `unknown` are empty.
4. `storage.suspectBindings()` lists texts that landed on a rail, border or status token outside a badge; rebind each to its role (`color.text`, `color.muted`, `color.faint`).
5. Export the board in dark. Text that vanishes or stays light marks a wrong role or an unbound shape: fix the rule in `helpers.js`, not only the shape, so the next page binds right.

## Cleaning up a page

For a page far over the page budget, with outdated boards or explicit dark boards:

1. List the boards with their shape counts and the mapping rows of the page (`docs/design-workflow.md`). Group them by topic so each group stays within the page budget; propose the split, deletions and merges to the user. Explicit dark boards go: the theme previews dark.
2. The user creates the pages `<Section> - <Topic>` and moves the boards (cut and paste in the PenPot UI). Check every page afterwards with `storage.checkPages`, giving it the planned boards per page: done when nothing is missing, extra or duplicated and no board overlaps.
3. Save a version first (`await penpot.currentFile.saveVersion(label)`): the file history may hold no autosave, and the plugin can't undo. Then cut every board that changes only one part to a partial board with `storage.trimToOverlay`: dialog or modal, menu with its column, the changed column (`Details`, `Transcript`, `Main`) or a nested panel. Keep one full-screen board per page for context. Export the boards with an overlay: a menu clipped or hidden behind its base means a wrong `keep` order.
4. Bind the colours of every page with `storage.bindPage`, one page per call ("Binding colours").
5. Update the page table and the page and board names in the mapping of `docs/design-workflow.md`; mark partial boards in the Data column.

## API gotchas

| Symptom | Cause | Handling |
|---|---|---|
| MCP calls time out; a proxy answers 502 | The PenPot tab froze (heartbeat lost) or the connection dropped | Ask the user to bring the tab to the foreground and reconnect the plugin. Keep calls small. Check what the last call wrote before you repeat it. |
| `storage.tree` is missing | Plugin storage resets on reconnect, tab reload or browser switch | Install the helpers again. Find shapes again by name (`storage.board`), not by ids from earlier calls. |
| `waitForLayoutUpdate` times out after a large clone | Layout of a big board takes longer | The write usually happened: query the result instead of repeating the call. |
| Token binding of a board times out or binds a few shapes per minute | `waitForLayoutUpdate` after each `applyToken` stalls | Apply all, then wait once: `storage.bindColours`, `storage.applyTokenEach`. After a timeout, `storage.unboundColours` shows what is left. |
| Some cloned shapes stay unbound after `token.applyToShapes([...])` | Batch token application skips shapes silently | `storage.applyTokenEach`. |
| A token binding disappears after `applyToken`; the raw colour stays | Applying a token the shape already carries removes the binding (a toggle); `shape.tokens` shows a new binding only after a moment | The helpers skip bound shapes and wait 2.5 s before they check. Never re-apply "to be sure". |
| Token bindings vanish | The page changed before PenPot applied them | Bind through `storage.bindPage` or `storage.applyTokenEach`, which wait before they return; in your own loops, wait before you switch the page. |
| A call over several pages times out, though each page alone works | The call runs past the MCP time limit (about a minute) | Writes: one page per call. Reads: about ten pages per call. |
| `openPage` fails with "Expected a Page object or a page UUID string"; `page.root` is undefined | `penpotUtils.getPages()` returns descriptors, not pages, and only the open page has a `root` | `await penpot.openPage(descriptor.id)`, then read `penpot.currentPage.root`. |
| A board's own fill stays unbound | `penpotUtils.findShapes(pred, board)` skips `board` itself | Check `[board, ...findShapes(...)]`, as `storage.unboundColours` does. |
| `board.removeFlexLayout is not a function` | The flex layout is removed through its object | `board.flex.remove()`. |
| A child doesn't move in a flex layout, or an inserted component lands in the wrong place | In flex layouts the `children` array is the visual order; `insertChild(i)` didn't reorder, and `setParentIndex` counts from the end of the child list | Call `appendChild` for each child in visual order, or assign `setParentIndex(length - 1 - position)`. |
| A new text renders in the default family or weight although `fontFamily`/`fontWeight` are set on the shape | The font needs its variant (`fontVariantId`) on the text range | `storage.text(...)`. |
| A `fill` child stays at its old width after `appendChild` or after a clone moved into a narrower parent; inputs overflow | The sizing was set before the parent had its width, and isn't recomputed | After moving, set `horizontalSizing = "fix"`, `resize` to the target width, then `"fill"` again; check the widths in the result. |
| A text loses its font after a range edit | `getRange(...).fontWeight = …` can reset the family | Set `fontFamily` and `fontId` on the full range afterwards; compare with a sibling text. |
| Initials or a badge label sit off-centre | An auto-width text box centred by flex leaves the glyphs off-centre | `storage.centreLabel`. |
| `export_shape` fails with "Cannot read properties of null" | It takes a shape id; `selection` needs a selected shape | Pass the id returned by your script. |
| `export_shape` fails with "Input Bit Array is empty" | The shape's page is not the open one | `penpot.openPage` its page first, then export. |
| A translucent fill turns opaque after binding | `applyToken` resets the fill opacity | Bind a separate rectangle at full fill opacity and set its layer `opacity` (the tinted row of the narrow events table). |
| Overlays (menu, popover, scrollbar thumb) vanish behind a panel after a cut | Removing a flex layout reverses the stacking of the children | `storage.trimToOverlay` restacks in `keep` order; for older cuts call `bringToFront()` on the overlay. |
| Library tokens don't apply | Tokens of a connected library can't be applied in a consuming file | Keep components and tokens in `App Design`. |
