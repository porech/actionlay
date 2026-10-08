// Prevent the browser from navigating to a dropped file; reuse the normal opener.
export function initializeVideoDrop({ open, canOpen, hint, reject }) {
  let depth = 0;
  const isFileDrag = event => Array.from(event.dataTransfer?.types ?? []).includes('Files');
  const reset = () => { depth = 0; hint.hidden = true; };
  document.addEventListener('dragenter', event => {
    if (!isFileDrag(event)) return;
    event.preventDefault(); depth++;
    hint.hidden = !canOpen();
  });
  document.addEventListener('dragover', event => {
    if (!isFileDrag(event)) return;
    event.preventDefault();
    event.dataTransfer.dropEffect = canOpen() ? 'copy' : 'none';
  });
  document.addEventListener('dragleave', event => {
    if (!isFileDrag(event)) return;
    if (--depth <= 0) reset();
  });
  document.addEventListener('drop', event => {
    if (!isFileDrag(event)) return;
    event.preventDefault(); reset();
    if (!canOpen()) return;
    const file = Array.from(event.dataTransfer.files).find(file =>
      file.type.startsWith('video/') || /\.(mp4|mov|m4v|webm|ogv|ogg|mkv|avi|mts|m2ts|lrv|insv)$/i.test(file.name));
    if (file) open(file);
    else reject();
  });
  document.addEventListener('dragend', reset);
  window.addEventListener('blur', reset);
}
