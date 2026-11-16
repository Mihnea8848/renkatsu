# Renkatsu Studio: editor design

This document fixes the direction of the editor before any of it is built.
It covers what the editor is for, how it is laid out, how its features are
reached, and how it looks. Values marked "to measure" are taken from
screenshots of Unity 6 and Unreal Engine 5.8 during the shell prototype and
filled in here; nothing in this document is meant to be guessed.

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

Every feature belongs to one of these. They decide the ribbon tabs, the
panels and the order of the roadmap.

| Category | What it covers | Where it lives |
|---|---|---|
| Project and library | The characters a project owns, assets, scenes, renders | Content browser, Library |
| Scene | Characters, items, maps, lights, cameras, folders | Outliner, Properties |
| Character | Look, outfits, accessories, plugin modules; later the Character Maker | Character tab, Properties |
| Pose | FK and IK, pose library, hands, face, gaze | Pose tab, Pose library |
| Animate | Timeline, keys, clips (later phase) | Animate tab, Timeline |
| Light and look | Lights, Studio effects, Koikatsu or modern look, quality | Light tab, Properties |
| Shots and bake | Cameras per shot, the Ren'Py link, bake queue | Bake tab, Shots |
| Tools and settings | Preferences, modules, keyboard map | View tab, Preferences window |

## Window layout

Unity's arrangement:

```
+--------------------------------------------------------------+
| Menu bar: File  Edit  Window  Help                           |
| Ribbon tabs: Home  Scene  Character  Pose  Animate  Light ... |
| Ribbon groups for the active tab                             |
+-------------------------------------------+------------------+
|                                           | Outliner         |
|                                           |                  |
|              Viewport                     +------------------+
|                                           | Properties       |
|                                           |                  |
+-------------------------------------------+                  |
| Content browser | Console | Timeline      |                  |
+-------------------------------------------+------------------+
| Status bar                                                   |
+--------------------------------------------------------------+
```

- Panels are docked edge to edge with a thin divider, as in Unity and Unreal.
- Every panel can be moved, tabbed with another, split or floated. Layouts
  are saved per user, and a project can carry its own.
- Window, in the menu bar, opens any panel whatever the ribbon tab.
- Several viewports can be open at once (a free camera and a shot camera, for
  example).

## Panels

| Panel | Purpose |
|---|---|
| Viewport | The live scene. Toolbar on top: camera, shading mode, gizmo space, snapping, stats |
| Outliner | The scene tree: folders, characters with their parts, items, lights, cameras |
| Properties | Everything about the selection, in collapsible sections |
| Content browser | The project's folders: characters, poses, maps, items, scenes, renders |
| Library | The character library: cards the project owns, with their scenes |
| Console | Log, warnings from loading cards, missing mods |
| Timeline | Keys and clips (later) |
| Pose library | Saved poses with thumbnails |
| Shots | The shot list for baking and the Ren'Py link |
| Modules | Plugin modules switched on or off, per project and per character |

## Ribbon

The ribbon replaces deep menus: one tab per category, each with labelled
groups of buttons, in the manner of Office and Roblox Studio. It can be
collapsed to its tab row. The active tab follows the task: selecting a
character offers Character and Pose.

| Tab | Groups |
|---|---|
| Home | Project (new, open, save); Edit (undo, redo, duplicate, delete); Tools (select, move, rotate, scale, local or world, snapping); Add (character, item, light, camera, map, folder); Physics (play, pause, reset) |
| Scene | Add; Organise (group, parent, hide, lock); Map (load map, environment) |
| Character | Card (load, replace, reload); Outfit (outfit, clothing states, shoes); Accessories; Expression (eyes, mouth, brows, gaze); Modules; Maker (later) |
| Pose | Library (load, save); Mode (FK, IK); Edit (mirror, reset, copy, paste); Hands; Look at camera |
| Animate | Timeline, keys, clips (later) |
| Light | Lights (add, key light); Environment; Effects (ambient occlusion, bloom, depth of field, vignette, colour filter); Look (Koikatsu, modern); Quality |
| Bake | Shots (new shot from view); Render (still, video); Queue; Ren'Py (export, sync) |
| View | Panels; Layouts (reset, save, load); Viewport (grid, gizmos, stats); Theme |

## Viewport

- Navigation follows Unity: Alt and left drag to orbit, middle drag to pan,
  wheel to zoom, right drag with W A S D to fly, F to frame the selection.
- Gizmos for move, rotate and scale; FK and IK handles in the Pose tab.
- Toolbar inside the viewport, top edge, as in Unreal: camera, shading,
  gizmo space, snapping, stats.

## Keyboard map

Unity's defaults where they exist: Q select, W move, E rotate, R scale,
F frame, Ctrl S save, Ctrl Z undo, Ctrl Y or Ctrl Shift Z redo, Ctrl D
duplicate, Delete delete, H hide. Every action can be rebound.

## Look

- Colours follow Unreal Engine 5: layered dark greys for panels, near-black
  recessed fields and viewports, blue for selection and focus, orange used
  sparingly for active and playing states. Exact values: to measure.
- Square panel edges. Small controls get at most the radius the reference
  editors use (to measure, 1 to 2 px). No shadows except under menus and
  popups.
- One outline icon font, one interface font, one monospace font for the
  console. Sizes: to measure.
- Metrics to measure from Unity 6 and Unreal 5.8: tab height, ribbon height,
  field height, row height in trees, panel padding, divider width, font sizes,
  icon sizes.
- Dark theme first; a light theme later from the same tokens.

## Toolkit

egui with egui_dock for docking and egui-wgpu to draw the viewport on the
renderer's own device. The theme is a set of tokens applied to egui's style,
so variants can be compared side by side in the running app.

## Order of work

1. This document.
2. The UI shell: layout, ribbon, menu bar and panels with placeholder content,
   a still image in the viewport, and two or three theme variants inside the
   Unreal scheme to choose from on screenshots.
3. wgpu 30 in the renderer.
4. The renderer drawing into the editor's texture every frame.
5. The live viewport.
6. Outliner, Properties and Content browser on a scene model.
7. Project and scene files, the character library.
8. Posing, then the other categories in roadmap order.
