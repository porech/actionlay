import { t } from './i18n.js';
const $ = id => document.getElementById(id);
function timeLabel(seconds) {
  if (!Number.isFinite(seconds)) return '0:00';
  const whole = Math.max(0, Math.floor(seconds));
  return `${Math.floor(whole / 60)}:${String(whole % 60).padStart(2, '0')}`;
}
export function refreshPlayer() {
  const video = $('video');
  $('play').textContent = video.paused ? '▶' : '❚❚';
  $('play').setAttribute('aria-label', t(video.paused ? 'Play' : 'Pause'));
  $('mute').textContent = video.muted || video.volume === 0 ? '🔇' : '🔊';
  $('mute').setAttribute('aria-label', t(video.muted ? 'Unmute' : 'Mute'));
  $('playback-time').textContent = `${timeLabel(video.currentTime)} / ${timeLabel(video.duration)}`;
  $('seek').max = Number.isFinite(video.duration) ? video.duration : 0;
  $('seek').value = video.currentTime;
  $('volume').value = video.muted ? 0 : video.volume;
  const fullscreen = Boolean(document.fullscreenElement);
  $('fullscreen').setAttribute('aria-label', t(fullscreen ? 'Exit full screen' : 'Full screen'));
  $('fullscreen').title = t(fullscreen ? 'Exit full screen' : 'Full screen');
}
export function initializePlayer(onError) {
  const video = $('video'), stage = $('stage');
  const safely = fn => (...args) => Promise.resolve().then(() => fn(...args)).catch(onError);
  const togglePlay = async () => { if (video.src) video.paused ? await video.play() : video.pause(); };
  const toggleFullscreen = async () => {
    if (document.fullscreenElement) await document.exitFullscreen();
    else if (stage.requestFullscreen) await stage.requestFullscreen();
    else throw new Error(t('Full screen is unavailable in this browser.'));
  };
  $('play').onclick = safely(togglePlay);
  $('fullscreen').onclick = safely(toggleFullscreen);
  $('seek').oninput = () => { video.currentTime = Number($('seek').value); };
  $('mute').onclick = () => { video.muted = !video.muted; };
  $('volume').oninput = () => { video.volume = Number($('volume').value); video.muted = video.volume === 0; };
  for (const event of ['loadedmetadata','timeupdate','play','pause','volumechange','emptied']) video.addEventListener(event, refreshPlayer);
  let idleTimer;
  let mousePosition;
  const showControls = () => {
    clearTimeout(idleTimer);
    stage.classList.remove('player-idle');
    if (document.fullscreenElement !== stage) return;
    idleTimer = setTimeout(() => {
      if (document.fullscreenElement !== stage) return;
      // Hidden buttons must not retain keyboard focus over the video.
      if ($('player-controls').contains(document.activeElement)) stage.focus({preventScroll:true});
      stage.classList.add('player-idle');
    }, 3000);
  };
  stage.addEventListener('mousemove', event => {
    if (document.fullscreenElement !== stage) return;
    if (mousePosition?.[0] === event.clientX && mousePosition?.[1] === event.clientY) return;
    mousePosition = [event.clientX, event.clientY];
    showControls();
  });
  document.addEventListener('fullscreenchange', () => {
    mousePosition = undefined;
    showControls();
    refreshPlayer();
  });
  stage.ondblclick = event => { if (!event.target.closest('.player-controls') && video.src) safely(toggleFullscreen)(); };
  stage.onkeydown = event => {
    if (event.target !== stage && event.target !== video) return;
    if (event.key === ' ') { event.preventDefault(); safely(togglePlay)(); }
    if (event.key.toLowerCase() === 'f') { event.preventDefault(); safely(toggleFullscreen)(); }
  };
  refreshPlayer();
}
