# Gecko illustration study

Tool: built-in `imagegen`, transparent background.
Reference: owner's [original photograph](IMG_20261006_132445_1.jpg), included alongside this prompt.
Output: `gecko-study.png`, subsequently vectorized into `../gecko.svg`.

Prompt:

> A single gecko mascot illustration for the ActionLay desktop application icon.
> The photograph is the precise biological/color reference, not a pose to copy.
> Create a lively, dimensional 3/4 view of this same Mediterranean wall gecko:
> natural warm olive-brown/tan coloration, dark irregular mottled saddles,
> gently bumpy skin, long tapering tail with alternating brown and cream rings,
> four distinct short bent limbs with five rounded adhesive toe pads.
> Its head is raised and turned toward the viewer: an unmistakably visible snout,
> warm expressive amber eyes with vertical slit pupils, alert curious relaxed
> expression, slight closed-mouth smile. The animal is supported by its bent
> limbs, has a rounded three-dimensional chest and body, clearly alive and awake.
> Whole gecko including full tail, filling a square canvas with healthy margins.
> Head and shoulders are prominent, toward upper right; tail curves to lower left
> then back in a graceful C. Polished semi-realistic 3D mascot illustration,
> clean sculptural volumes, subtle shallow scale texture, clear highlights and
> soft self-shadows; simplified details suitable for icon scaling and SVG tracing.
> Keep the animal's natural colours and anatomy. Transparent background.
> No wall, floor, pedestal, app tile, disk, typography, branding or watermark.
> No extra limbs/toes. Warm soft light from upper left and slight rim lighting.

Vectorization: VTracer 0.6.15, spline/stacked colour paths, filter_speckle=10,
color_precision=6, layer_difference=12, corner_threshold=60, length_threshold=4,
max_iterations=10, splice_threshold=45, path_precision=1. Alpha coverage is
quantized at 128 before tracing because VTracer treats any nonzero alpha as
opaque. The composed icon sources are independent of the raster study.
