# M4 editor implementation and verification

The editor works without a video, with synthetic telemetry, or over the current
video. All sixteen native widgets can be added, styled, moved and resized.
Schema-driven properties retain unknown fields and offer reset controls.
Coverage badges identify unavailable or partial metrics in real-video previews.

Multiple selection supports Ctrl/Command or Shift and select-all. Group,
ungroup, duplicate, delete, clipboard and reparenting preserve geometry; grouping
also preserves inherited opacity. Invalid cycles are rejected. Ungrouping an
unknown extended group is refused to avoid discarding its fields. Single-widget
resizing and paint-order controls remain available. Properties edit the primary
selection. Each drag is one undo operation.

Optional snapping uses edges and centres of other widgets and the preview root,
with an eight-screen-pixel tolerance and yellow guides. Alt temporarily bypasses
snapping. Multiple selections move as one bounding box. Automatic choice among
the nine anchors on drop is separately optional.

Installed and attached fonts are resolved per layout. Packaged faces take
priority, with embedded Roboto as fallback and notices for unavailable families
or weights. Font changes invalidate shaped text and static rendering caches.
Fonts are never installed globally. Export can include used custom faces after
a redistribution reminder, or omit all fonts while retaining family references.

Portable `.actionlay-layout` files are ZIP archives containing `layout.json`
and declared assets. The `assets` array at the JSON root lists relative paths.
Attached fonts and images are carried as bytes; preview backgrounds remain
editor aids, and there is no new image widget. Future fields survive round trips;
external files must be declared explicitly. Imported packages receive a unique
copy in the local library. Asset-bearing layouts require package saves instead
of JSON saves that would lose their assets.

Package reads reject unsafe paths, duplicate names, symlinks, missing declared
files and excessive sizes (32 MiB per asset, 128 MiB total, 256 entries). They do
not extract files. Saves validate first and atomically replace a neighbouring
temporary file. Failed saves retain the editor's saved baseline. Dirty drafts
are guarded on replacement and exit.

Windows automated tests exercise full egui drag/release gestures, multiselection,
undo, cross-parent geometry and opacity, cycle rejection, snapping, packages,
font scope and fallback, and invalid/failed saves. Existing renderer pixel tests
remain part of the checks. Native desktop checks verified adding widgets,
select-all, grouping/ungrouping and package export; the exported ZIP was inspected.
The desktop mouse automation did not reliably produce widget movement, so native
manual drag verification remains pending. Linux and macOS were not tested locally.

Build and check logs accompany the local executable. The roadmap remains open
until the remaining manual interaction check is resolved.
