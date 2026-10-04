# M0 – Esito del prototipo player

Data: 2026-10-04 · Commit misurato (codice): 0c4de1d, branch `m0-player`; il report e' un commit successivo · Host: Apple M4 Max, macOS (Darwin 27.0.0)

Stato del gate: **parziale**. Le misure automatiche sono fatte; le verifiche visive, A/V a orecchio e Windows sono **PENDING — user check** (chi ha scritto il report non puo' vedere la finestra ne' sentire l'audio, e non ha una macchina Windows). Nessun risultato e' stato inventato.

## Decodifica (decode-bench, release, misurato)
| File | Piattaforma | Sorgente | HW fps | SW fps | Backend HW |
|---|---|---|---|---|---|
| GX013370.MP4 (1920x1440 @100 HEVC, 19866 frame) | macOS M4 Max | 100 | 166.5 | 515.9 | videotoolbox |
| hevc10-2160p60-sync.mp4 (3840x2160 @60 HEVC 10 bit) | macOS M4 Max | 60 | 165.3 | 209.0 | videotoolbox |
| hevc8-1440p100-sync.mp4 (1920x1440 @100) | macOS M4 Max | 100 | 186.2 | 1662.8 | videotoolbox |
| h264-1080p30-44k.mp4 (1080p30) | macOS M4 Max | 30 | 227.0 | 1968.7 | videotoolbox |
| (tutti) | Windows | | PENDING — user check | PENDING | d3d11va atteso |

Criterio "HW >= fps sorgente": rispettato su tutti i file macOS (1.65x–7.6x). Nota: i campioni sintetici hanno contenuto banale, quindi i loro fps SW non sono rappresentativi; il dato realistico e' GX013370.MP4.

Osservazione importante: su Apple Silicon la decodifica software e' **piu' veloce** di VideoToolbox (516 contro 166 fps sul file reale) per via del download GPU->CPU di ogni frame. L'accelerazione hardware qui fa risparmiare CPU/energia, non throughput. Il margine HW resta comunque > 1.6x il frame rate sorgente.

## Binario (misurato)
- Collegamento statico: **ok**. `otool -L target/release/actionlay` non elenca alcuna libav*/libsw*; solo framework di sistema (AppKit, ApplicationServices, CoreGraphics, CoreVideo, Carbon, CoreFoundation, Foundation, QuartzCore, Metal, VideoToolbox, CoreMedia, CoreServices, AudioToolbox, AVFoundation, Security, OpenCL, OpenGL, VideoDecodeAcceleration, CoreAudio, ColorSync) e libSystem, libobjc, libiconv. Nessuna libreria non di sistema.
- Dimensione: 19 919 568 byte (19.0 MiB) in release; 16 527 584 byte (15.8 MiB) dopo `strip` (copia). `decode-bench`: 5.06 MB.

## Latenze (release, misurate; HEVC, audio attivo, 5 ripetizioni)
Tempo dal comando al primo frame consegnato a `poll_frame`. Open = `Player::open` -> primo frame. Seek = seek preciso verso punti oltre i 5 s del file. Resume = `pause()` + `play()` con audio (re-seek R2) -> primo frame.

| File | Backend | Open | Seek preciso (min–max) | Resume (min–max) |
|---|---|---|---|---|
| hevc8-1440p100-sync | videotoolbox | 140 ms | 8 ms – 1.44 s | 8 ms – 930 ms |
| hevc8-1440p100-sync | software | 18 ms | 14 – 155 ms | 5 – 105 ms |
| GX013370.MP4 | videotoolbox | 15 ms | 7 – 299 ms | 6 – 295 ms |
| GX013370.MP4 | software | 58 ms | 66 – 194 ms | 54 – 171 ms |

Lettura: i valori dipendono dalla distanza dal keyframe precedente (seek preciso e R2 ridecodificano da li'). Intervallo keyframe misurato con ffprobe: GX013370.MP4 un keyframe ogni 0.5 s; hevc8-1440p100-sync.mp4 keyframe a 0, 2.5, 5.0 s (GOP di 2.5 s, 5x piu' lungo). Sul file reale il caso peggiore e' ~0.3 s. Sul sintetico con VideoToolbox arriva a 0.9–1.4 s: e' coerente con il GOP piu' lungo, e il confronto col software (<= 155 ms sullo stesso file) fa pensare, probabilmente, che il costo per frame del readback GPU->CPU pesi sulla ridecodifica dal keyframe; la causa non e' stata isolata con una misura dedicata. Le ripetizioni singole non sono riportate. Il test temporaneo usato per la misura e' stato rimosso (non committato).

## Qualita' del codice (misurato)
- `cargo test --workspace -- --test-threads=1`: tutti i test passano (0 falliti, vedi output in task-10-report.md).
- `cargo fmt --all --check`: ok. `cargo clippy --workspace --all-targets -- -D warnings`: pulito.

## Riproduzione — PENDING — user check (macOS)
Non verificabile dall'agente. Lanciare `source scripts/env.sh && ./target/release/actionlay samples/synthetic/hevc8-1440p100-sync.mp4` e compilare:

- [ ] PENDING — user check: **A/V.** Lampo bianco e beep di ogni secondo coincidono a occhio/orecchio; la statistica `A/V` resta entro ±40 ms per tutti i 10 s. Massimo osservato: ___ ms.
- [ ] PENDING — user check: **Frame scartati.** Su `hevc8-1440p100-sync.mp4` `dropped` cresce al massimo di ~40 frame/s su monitor a 60 Hz (100 -> 60 mostrati); su `h264-1080p30-44k.mp4` `dropped` **non** cresce. Osservato: ___
- [ ] PENDING — user check: **Colori vs ffplay.** Aprire lo stesso istante con `ffplay -ss 3 samples/synthetic/hevc8-1440p100-sync.mp4` e portare anche l'app a 3 s (seek), cosi' i due mostrano lo stesso istante; confrontare a vista (barre colorate, neri, bianchi). Se l'immagine e' piu' chiara/slavata applicare la correzione sRGB (Task 8 Step 3) e ripetere. Esito: ok / correzione applicata.
- [ ] PENDING — user check: ripetere A/V e colori con `samples/GX013370.MP4` (scena reale, full range): niente colori slavati.
- [ ] PENDING — user check: **Robustezza.** Seek rapidi ripetuti, seek a fine file, apertura di `samples/synthetic/hevc8-1080p30-noaudio.mp4`: nessun crash, nessun blocco. A fine file: si ferma sull'ultimo frame e resta in pausa; premendo play riparte dall'inizio.

## Windows — PENDING — user check
Serve una macchina Windows 10/11 con GPU (i runner CI non ne hanno). Copiare `actionlay.exe` e `decode-bench.exe` (artefatti CI o build locale, Task 2) e i campioni sintetici, poi:

- [ ] PENDING — user check: `decode-bench.exe` HW/SW sugli stessi file della tabella; atteso `decoder: d3d11va`, HW >= fps sorgente.
- [ ] PENDING — user check: `dumpbin /dependents actionlay.exe` non deve elencare `avcodec*.dll` (ne' altre DLL FFmpeg).
- [ ] PENDING — user check: ripetere i controlli A/V, scarti, colori, robustezza della sezione macOS.

## Decisione
**In attesa delle verifiche utente** (visive, A/V, Windows). Non e' ancora una decisione definitiva.

Raccomandazione basata solo sui dati automatici: **proseguire con egui + wgpu + FFmpeg statico**. Motivi: decodifica HW ben oltre il frame rate sorgente su tutti i file (anche 4K60 10 bit), collegamento statico verificato, binario di 16–19 MB, latenze di seek/resume sul file reale entro ~0.3 s, test/fmt/clippy puliti. Chiedo all'utente di giudicare a mano se il seek di 0.9–1.4 s con VideoToolbox sul file sintetico (GOP 2.5 s) e' accettabile.

Il ripiego su libmpv va considerato solo se le verifiche utente mostrano A/V fuori da ±40 ms, colori non correggibili, crash nei seek, o d3d11va non funzionante su Windows.
