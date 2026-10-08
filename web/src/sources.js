export function supportedActivity(file) { return /\.(gpx|fit|insgps)$/i.test(file.name); }

// Inspect with the shared WASM parser; selection never guesses from filesystem dates.
export async function planActivities(files, inspect, folder = false, videoUtc = null) {
  const strict = folder || files.length > 1;
  const candidates = [], errors = [];
  let knownTime = Boolean(videoUtc);
  for (const file of files.filter(supportedActivity)) {
    try {
      const summary = await inspect(file);
      knownTime ||= Boolean(summary.videoUtc);
      if (!strict || summary.matches) candidates.push({file, ...summary});
    } catch (error) { errors.push(`${file.webkitRelativePath || file.name}: ${error.message ?? error}`); }
  }
  if (candidates.length === 1) return {selected:candidates[0], candidates:[], errors};
  const message = candidates.length > 1 ? 'Several activities match. Choose one:'
    : strict && !knownTime ? 'Video time is unknown. Set Video UTC or select a single file.'
    : 'No activity matches the video time. Select a single file to align starts.';
  return {candidates, errors, message};
}
