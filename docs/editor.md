# Renkatsu Studio: editor design

This document fixes the direction of the editor. It covers what the editor
is for, how it is laid out, how its features are reached, and how it looks.
The shell prototype in `crates/rk-studio` follows it; where the prototype
still offers several designs to choose from, this document says so.

## Goals

- A familiar game editor: anyone who has used Unity, Unreal or Roblox Studio
  finds their way without a manual. CharaStudio users find its objects and
  settings under the names they know.
- Many features, few places to look. Unreal's depth without its maze of
  menus.
- A modern, polished look that does not read as generic generated design: no
  rounded cards, no pill buttons, no floating panels with gaps between them.
- Fast on mid-range hardware, with the live viewport as the centre of the
  app.

## Core categories

Every feature belongs to one of these. They decide the modes, the panels
and the order of the roadmap.

| Category | What it covers | Where it lives |
|---|---|---|
| Project and library | The characters a project owns, assets, scenes, renders | Content browser, Library |
| Scene | Characters, items, maps, lights, cameras, folders | Outliner, Properties |
| Character | Look, outfits, accessories, plugin modules; later the Character Maker | Character mode, Properties |
| Pose | FK and IK, pose library, hands, face, gaze | Pose mode, Pose library |
| Animate | Timeline, keys, clips (later phase) | Animate mode, Timeline |
| Light and look | Lights, Studio effects, Koikatsu or modern look, quality | Light mode, Properties |
| Shots and bake | Cameras per shot, the Ren'Py link, bake queue | Bake mode, Shots |
| Tools and settings | Preferences, modules, keyboard map | Menu row, Preferences window |

## Window layout

Unity's arrangement:

```
+--------------------------------------------------------------+
| Menus  Project > Scene      Play Pause Stop     Search Layout |
+-------------------------------------------+------------------+
| Selection bar                 View options | Outliner         |
| Modes | Tools |                            |                  |
|       |       |      Viewport              +------------------+
|       |       |                            | Properties       |
+-------------------------------------------+                  |
| Content browser | Console | Timeline      |                  |
+-------------------------------------------+------------------+
| Status bar                                                   |
+--------------------------------------------------------------+
```

- The viewport gets as much room as the other panels can spare.
- The menu row carries the menus, the project and scene, the physics play
  controls in its centre (a dark segmented group with a green play and a
  red stop, as in Unreal 5), the command search and the layout menu. There
  is no toolbar row and no ribbon under it.
- Panels are docked edge to edge with a thin divider, as in Unity and Unreal.
- Every panel can be moved, tabbed with another, split or floated. Layouts
  are saved per user, and a project can carry its own.
- Window, in the menu row, opens any panel whatever the mode.
- Several viewports can be open at once (a free camera and a shot camera, for
  example).

## Panels

| Panel | Purpose |
|---|---|
| Viewport | The live scene, with the modes, their tools, the transform tools, the selection bar and the view options drawn over it |
| Outliner | The scene tree: folders, characters with their parts, items, lights, cameras |
| Properties | Everything about the selection, in collapsible sections |
| Content browser | The project's folders: characters, poses, maps, items, scenes, renders |
| Library | The character library: cards the project owns, with their scenes |
| Console | Log, warnings from loading cards, missing mods |
| Timeline | Keys and clips (later) |
| Pose library | Saved poses with thumbnails |
| Shots | The shot list for baking and the Ren'Py link |
| Modules | Plugin modules switched on or off, per project and per character |

## Modes and tools

Unreal's modes replace deep menus and any ribbon: one mode per category,
shown as a vertical rail of icons with their names. The active mode's tools
open beside the rail as a palette of named tools in sections. Rail and
palette are drawn over the viewport; they are not docked panels.

| Mode | Tools |
|---|---|
| Select | Pick (objects, box select, all of a type); Filter (characters, items, lights and cameras) |
| Character | Card (load, replace, reload); Wear (outfit, shoes, accessories); Face (expression, gaze) |
| Pose | Edit (FK, IK, mirror); Library (load, save, reset); Body (hands, expression, look at camera) |
| Light | Lights (add, key light); Effects (ambient occlusion, bloom, depth of field); Look (Koikatsu, modern) |
| Animate | Keys (set, auto key, delete); Clips (load clip, bake physics) |
| Bake | Shots (new shot from view, shot list); Render (still, video, queue); Ren'Py (export, sync) |

Three more things lie over the viewport:

- The transform tool strip, as Unity's tool overlay: view, select, move,
  rotate, scale, pose.
- The selection bar: the selected object and the actions that fit its kind
  (a character offers Load card, Outfit, Expression, Modules). In the
  default design it sits in the viewport's header row, not over the
  picture, and falls back to a bar along the viewport's top edge when the
  viewport has no header row.
- The view options: camera, shading, snapping.

All of it gets out of the way:

- Choosing the active mode again, or the palette's close button, folds the
  palette. Unpinned, the palette also folds when work starts in the
  viewport.
- The rail's chevron shrinks the rail to icons; the names move to tooltips.
- The backtick key, the eye button or View in the menu row hides every
  overlay.
- Shift and Space, or the corner button, gives the viewport the whole
  window.
- The keys 1 to 6 pick a mode.

## Viewport

- Navigation follows Unity: Alt and left drag to orbit, middle drag to pan,
  wheel to zoom, right drag with W A S D to fly, F to frame the selection.
- Gizmos for move, rotate and scale; FK and IK handles in Pose mode.
- View options inside the viewport, as in Unreal: camera, shading,
  snapping.

## Keyboard map

Unity's defaults where they exist: Q select, W move, E rotate, R scale,
F frame, Ctrl S save, Ctrl Z undo, Ctrl Y or Ctrl Shift Z redo, Ctrl D
duplicate, Delete delete, H hide. Every action can be rebound.

## Look

- Colours follow Unreal Engine 5: input #0F0F0F, background #151515, panel
  #242424, header #2F2F2F, dropdown #383838, hover #575757, text #C0C0C0,
  selection #0070E0, and orange used sparingly for unsaved, warning and
  playing states. The values come from memory of Unreal's style code and
  were refined by eye; they live in `crates/rk-studio/src/theme.rs`.
- Square panel edges. Small controls get a radius of 3 px at most. No
  shadows except under menus and popups, and nothing floats as a card.
- An object's kind shows as its icon in the kind's colour, from a muted
  set; the text beside it stays neutral.
- Open Sans in two weights for the interface, Noto Sans Mono for the
  console, Phosphor for icons. Body text is 12 to 13 px.
- Axes are marked red, green and blue in number fields.
- Dark theme first; a light theme later from the same tokens.

## Toolkit

egui with egui_dock for docking and egui-wgpu to draw the viewport on the
renderer's own device. The theme is a set of tokens applied to egui's style,
and a design is a set of sizes and arrangements in
`crates/rk-studio/src/design.rs`, chosen with `--design`. The default, D,
combines the three designs compared before it: the filled panel tabs with
icons and the Details rows of A, the floating blocks of B over the viewport
(tool strip, mode rail with its palette beside it, view options), the
Outliner of C with indent guides and visibility at the right edge, and the
selection bar in the viewport's own header row beside its tab. A, B and C
stay available for comparison.

## Order of work

1. This document.
2. The UI shell: layout, menu row, modes and panels with placeholder content,
   a still image in the viewport, and two or three complete designs inside
   the Unreal scheme to choose from on screenshots.
3. wgpu 30 in the renderer.
4. The renderer drawing into the editor's texture every frame.
5. The live viewport.
6. Outliner, Properties and Content browser on a scene model.
7. Project and scene files, the character library.
8. Posing, then the other categories in roadmap order.
