// tiny-skia returns premultiplied RGBA; Canvas ImageData expects straight alpha.
export function straightRgba(bytes) {
  const pixels = new Uint8ClampedArray(bytes.length);
  for (let i = 0; i < bytes.length; i += 4) {
    const alpha = bytes[i + 3];
    pixels[i + 3] = alpha;
    if (alpha) for (let channel = 0; channel < 3; channel++) pixels[i + channel] = Math.round(bytes[i + channel] * 255 / alpha);
  }
  return pixels;
}
export function drawOverlay(core, context, time, width, height) {
  context.putImageData(new ImageData(straightRgba(core.render(time, width, height)), width, height), 0, 0);
}
