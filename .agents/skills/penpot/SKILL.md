---
name: penpot
description: Drawing and changing boards in the PenPot file "App Design" through the PenPot MCP. Use before the first `execute_code` call of a session, when creating or changing boards, binding colours to design tokens, exporting a board to check it, or when PenPot calls time out, writes fail or the plugin reconnects.
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
| `await storage.applyTokenEach(tokenName, shapes, props = ["fill"])` | Binds a token shape by shape; returns the names still unbound |
| `await storage.centreLabel(shape)` | Centres the text child of an avatar or badge, keeping weight and fill token |

## Before the first write

1. Run "Check the setup" from `docs/design-workflow.md`: `penpot.currentFile.name` is `App Design` and `penpotUtils.getPages()` lists the pages of the page table. Install the helpers.
2. Open the page you will write to (`await penpot.openPage(page)`) and measure it with `storage.pageBudget()`. Keep the **page budget**: about 1,000 shapes per page. A heavy page makes the browser tab freeze, and calls then time out or writes fail half-way. One page with ~9,500 shapes froze on every export. A cloned 1440 px board costs 300–600 shapes.
   - If the new boards would push the page past the budget, don't add to the full page. The plugin API can neither clone nor move a shape to another page, so ask the user to create a new page `<Section> - <Topic>` and paste a copy of the nearest board onto it (copy it in the PenPot UI on the full page, paste it on the new page). Add the page to the page table in `docs/design-workflow.md`, then draw on the new page, cloning from that copy.
   - Delete the empty icon backgrounds (`base-background` rectangles) of every board you clone.
3. Plan the fewest boards that show every state the spec names. One board per visually different state. Show states that differ only in one control inside one board, or as a small partial board (the part that changes, not the whole screen).

## Drawing a board

1. Clone the nearest existing board (`board.clone()`), name it `<View> · <state>`, and place it in free space next to the board it derives from.
2. Change the clone; don't redraw what exists. Detach a component copy (`shape.detach()`) only where you change its structure.
3. Bind every colour to a token of the sets `light` and `dark` with `storage.applyTokenEach`. A new colour becomes a token in both sets.
4. Work in small calls: one board, or one part of a board, per `execute_code`.
5. Export each board with `export_shape` (by shape id) and look at it. For the dark preview, switch the theme of group `Mode` to `Dark`, export, and switch back to `Light`.
6. Check every touched page with `storage.overlaps()`.

Done when every new board is exported in light and dark, no new board overlaps another, the theme is back on `Light`, and every touched page is within the page budget.

## API gotchas

| Symptom | Cause | Handling |
|---|---|---|
| MCP calls time out; a proxy answers 502 | The PenPot tab froze (heartbeat lost) or the connection dropped | Ask the user to bring the tab to the foreground and reconnect the plugin. Keep calls small. Check what the last call wrote before you repeat it. |
| `storage.tree` is missing | Plugin storage resets on reconnect, tab reload or browser switch | Install the helpers again. Find shapes again by name (`storage.board`), not by ids from earlier calls. |
| `waitForLayoutUpdate` times out after a large clone | Layout of a big board takes longer | The write usually happened: query the result instead of repeating the call. |
| Some cloned shapes stay unbound after `token.applyToShapes([...])` | Batch token application skips shapes silently | `storage.applyTokenEach`. |
| A token binding disappears after `applyToken`; the raw colour stays | Applying a token the shape already carries removes the binding (a toggle) | `storage.applyTokenEach`, which skips bound shapes. Never re-apply "to be sure". |
| Token bindings vanish | The page changed before PenPot applied them | Check `shape.tokens` before you switch the page. |
| A child doesn't move in a flex layout, or an inserted component lands in the wrong place | In flex layouts the `children` array is the visual order; `insertChild(i)` didn't reorder, and `setParentIndex` counts from the end of the child list | Call `appendChild` for each child in visual order, or assign `setParentIndex(length - 1 - position)`. |
| A text loses its font after a range edit | `getRange(...).fontWeight = …` can reset the family | Set `fontFamily` and `fontId` on the full range afterwards; compare with a sibling text. |
| Initials or a badge label sit off-centre | An auto-width text box centred by flex leaves the glyphs off-centre | `storage.centreLabel`. |
| `export_shape` fails with "Cannot read properties of null" | It takes a shape id; `selection` needs a selected shape | Pass the id returned by your script. |
| Library tokens don't apply | Tokens of a connected library can't be applied in a consuming file | Keep components and tokens in `App Design`. |
