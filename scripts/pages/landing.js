// Detection changes the recommendation only; every platform stays accessible.
const release = document.querySelector('#download-note').textContent.match(/release (\S+)/)?.[1];
const platform = navigator.userAgentData?.platform || navigator.platform || '';
const agent = navigator.userAgent || '';
const mobile = /Android|iPhone|iPad|iPod/i.test(agent) || (/Mac/i.test(platform) && navigator.maxTouchPoints > 1);
const download = document.querySelector('#download');
const note = document.querySelector('#download-note');
const base = 'https://github.com/porech/actionlay/releases/latest/download/';
if (!mobile && release) {
  if (/Mac/i.test(platform)) {
    download.textContent = 'Download for macOS ↓';
    download.href = `${base}actionlay-${release}-macos-universal.dmg`;
    note.textContent = `Release ${release} · Universal DMG · Apple Silicon & Intel · macOS 12+`;
  } else if (/Win/i.test(platform)) {
    download.textContent = 'Download for Windows ↓';
    download.href = `${base}actionlay-${release}-windows-x64-setup.exe`;
    note.textContent = `Release ${release} · 64-bit installer · Windows 10/11`;
  } else if (/Linux/i.test(platform)) {
    download.textContent = 'Install on Linux →';
    download.href = 'packages/';
    note.textContent = `Release ${release} · 64-bit · APT / DNF · Ubuntu 22.04+, Debian 12+ & compatible distributions`;
  }
}
