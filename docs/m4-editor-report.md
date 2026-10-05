# M4: first editor implementation

The desktop toolbar and Layout menu open the editor or create an empty layout
without a video. The editor supports video preview, static preview formats,
custom dimensions and PNG/JPEG/WebP/BMP backgrounds. Static previews use isolated
synthetic telemetry and an offline tile store; real-video previews share the
player's map cache and use its telemetry.

The palette includes all sixteen native widget types. Layers select nested
widgets, and the render traversal supplies measured selection boxes including
text. Dragging stores offsets relative to the parent; resizing adjusts widget
parameters. Root-edge/centre snapping, anchor guides, duplicate, delete, paint
order, clipboard and bounded undo/redo are available. Properties are generated
from the layout schema, with per-field and style-group reset controls. Unknown
fields survive edits and copies. Missing/partial metric coverage is shown for
real-video previews.

Editor drafts are independent of the player layout. Save and Save As both choose
a destination for new/system layouts; successful saves establish a user file.
Saving validates the draft, synchronises a neighbouring temporary file and
atomically replaces the destination. Failed writes do not change the baseline.
Exit, quit/window close and layout replacement guard dirty drafts with save,
discard and cancel. Cancelled or failed saves leave the editor open.

Automated checks exercise palette rendering without video, complete mouse-drag
and release gestures, single-gesture undo, nested relative positioning, recursive
ID removal on duplication, unknown-field preservation and failed/invalid saves.
Render tests verify text selection geometry and unchanged preview pixels when
collecting hit boxes. Desktop window inspection remains unverified locally due
to the existing Computer Use capture failure.

M4 is still in progress. Multiple selection, reparenting/grouping UI, snapping to
other widgets, full font resolution and self-contained layout packages remain.
This iteration saves validated `.ovl.json` files and uses embedded Roboto;
requested custom font references are preserved. Background preview images are
editor aids, not assets embedded in the layout.
