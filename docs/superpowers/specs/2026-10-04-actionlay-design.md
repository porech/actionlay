# ActionLay — Design

- **Data**: 2026-10-04
- **Stato**: approvata (2026-10-04)
- **Nome**: ActionLay (provvisorio; libero su GitHub e crates.io alla data)
- **Licenza**: GPL-3.0-or-later

## 0. Obiettivo e criteri di successo

Applicazione desktop **open source e multipiattaforma** (Windows, macOS, Linux) per:

1. **riprodurre** video di action camera con un dashboard di telemetria sovrapposto e sincronizzato in tempo reale;
2. **creare e modificare** il dashboard in un editor visuale (aggiungere, posizionare, ridimensionare, configurare elementi);
3. **esportare** il video con l'overlay, oppure il solo overlay trasparente.

È ispirata a [gopro-dashboard-overlay](https://github.com/time4tea/gopro-dashboard-overlay) (time4tea, GPL-3.0), che viene citato nei credits come fonte di ispirazione e know-how; i suoi layout vengono importati e convertiti nel formato di ActionLay. Non c'è compatibilità diretta con il formato XML originale.

**Vincoli**
- Scritto in **Rust**.
- Distribuito come **singolo eseguibile** per piattaforma, senza dipendenze da installare (eccezioni: driver GPU del sistema; su macOS un bundle `.app`).

**Criteri di successo**
- Ogni layout dell'originale, importato, viene renderizzato **quasi identico** all'originale (misurato con immagini di riferimento, §8).
- Un video GoPro 4K/H.265 (e il campione 1920×1440 a 100 fps) si riproduce **fluido** con overlay su un portatile recente, su macOS e Windows.
- Anteprima ed export sono **identici** (stesso motore di render).

## 1. Ambito della v1

### Incluso
- Player con overlay in tempo reale, seek preciso, frame avanti/indietro, velocità variabile.
- Editor visuale completo (§5).
- **Tutti i tipi di widget** dell'originale (§4.3) — elenco preso dal registro dei componenti di `layout_xml.py`, non solo dalla documentazione.
- Fonti dati: GPMF GoPro; GPX e FIT esterni sincronizzati sull'orario; altre camere (DJI, Insta360, …) tramite telemetry-parser.
- Capitoli GoPro riconosciuti e uniti automaticamente; ogni file resta apribile da solo.
- Export: video finale (H.264/H.265) e solo overlay (ProRes 4444 / sequenza PNG con alpha); esportazione di un intervallo (punti in/out).
- Riga di comando per l'export (stessi crate dell'app).
- Zone di privacy (come nell'originale: i widget mappa non disegnano punti dentro le zone).
- Memoria delle preferenze e dello stato per video (§6); registrazione in "Apri con" su Windows (§6.3).

### Escluso dalla v1 (scelte esplicite)
- Posizionamento in percentuale nei layout (bastano ancore + scala uniforme).
- Audio a velocità ≠ 1x (viene silenziato).
- Timeline multi-clip / montaggio.
- Coda di export, scelta di bitrate/risoluzione di output avanzata.
- Firma del codice (macOS/Windows) e aggiornamento automatico (solo avviso di nuova versione).
- Video 360° a doppio flusso (GoPro MAX `.360`).
- Lettura/scrittura del formato XML originale oltre all'importazione una tantum.

## 2. Architettura

Workspace Cargo con crate a responsabilità singola:

| Crate | Responsabilità | Dipende da |
|---|---|---|
| `telemetry` | Lettura GPMF/DJI/Insta360 (telemetry-parser), GPX, FIT. Serie temporale unificata con interpolazione e metriche derivate. Allineamento al tempo del video; unione dei file esterni per orario UTC con scarto regolabile; conversione tempo-file ↔ tempo-reale per timelapse/TimeWarp. Zone di privacy. | — |
| `layout` | Modello del layout (albero di nodi), schema dei widget, (de)serializzazione JSON, validazione, importatore XML dell'originale. | — |
| `maps` | Provider di tile, download, cache su disco, prefetch dell'area del percorso. | rete (rustls) |
| `render` | Funzione pura: (layout, istante t, telemetria, dimensioni, scala) → immagine RGBA premoltiplicata. Calcola anche i riquadri di ogni nodo per l'editor. Motore unico per anteprima ed export. | `layout`, `telemetry`, `maps` |
| `media` | ffmpeg statico: apertura file e capitoli, decodifica HW, audio, codifica export. | ffmpeg |
| `app` | UI egui + wgpu: player, editor, export, preferenze. Include la CLI di export. | tutti |

**Flusso in riproduzione**: orologio del player → t → `telemetry` (valori in t) → `render` (overlay) → texture GPU → composizione nello shader sopra il frame video.

**Flusso in export**: per ogni frame decodificato → `render` dell'overlay al suo timestamp → composizione (o solo overlay) → encoder.

## 3. Player e sincronizzazione

**Thread**
- *Decodifica video*: ffmpeg demux + decodifica HW (VideoToolbox / D3D11VA / VA-API), fallback software multi-thread con avviso. Coda corta (~8 frame). Upload come texture YUV (NV12, P010 per 10 bit); conversione in RGB nello shader con matrice colore (BT.709/BT.2020) e range corretti (es. `yuvj420p` = full range).
- *Audio*: decodifica AAC → `cpal`. **L'audio è l'orologio master**. Senza audio o a velocità ≠ 1x: orologio di sistema, audio silenziato.
- *Presentazione*: a ogni vsync si mostra il frame con pts ≤ orologio più recente; i frame in ritardo si scartano (es. 100 fps su 60 Hz).
- *Render overlay*: thread separato dalla UI, ~25 Hz in riproduzione, immediato in pausa/dopo seek; doppio buffer. In anteprima si renderizza alla risoluzione di visualizzazione (scalando il layout), in export alla risoluzione piena.

**Tempo**
- Tutto è indicizzato sul **tempo del file** (pts). La telemetria GPMF è allineata ai pts video.
- **Capitoli**: timeline virtuale che concatena i file (offset dei pts); la telemetria è concatenata allo stesso modo. Aprendo `GX01xxxx` si caricano i capitoli successivi; aprendo un capitolo intermedio il file funziona da solo e si propone di caricare la sequenza dal primo.
- **Seek**: durante il trascinamento solo keyframe; al rilascio seek preciso al frame (decodifica dal keyframe precedente scartando gli intermedi). Frame ±1.

**Errori**: senza decodifica HW → software + avviso; senza telemetria → video normale, widget con "nessun dato"; file corrotti letti fin dove possibile.

## 4. Layout e render

### 4.1 Formato del layout (`*.ovl.json`)

JSON versionato con JSON Schema pubblicato.

- **Unità**: 1 unità = 1/1080 dell'altezza del video. I valori dei layout 1080p dell'originale si trasferiscono 1:1.
- **Intestazione**: `version`, `name`, `design_aspect` (es. `16:9`), sistema di unità predefinito (metrico/imperiale), font predefinito, colori di base.
- **Nodo**: `id`, `name`, `type`, `anchor` (9 valori: `top-left` … `bottom-right`), `offset` [x, y] dall'ancora, dimensione propria del widget, `opacity`, `visible`, parametri specifici.
- **Gruppi**: con dimensione esplicita i figli possono ancorarsi dentro il gruppo; senza dimensione i figli sono posizionati relativamente all'origine del gruppo (come i `composite` originali).
- **Dati e testo**: `metric`, `units`, `format` con sintassi propria (`"{value:.0}"`, `"{unit}"`, date con `strftime`).

Esempio:
```json
{ "id": "speed-main", "type": "metric", "anchor": "bottom-left", "offset": [16, -120],
  "metric": "speed", "units": "kmh", "format": "{value:.0}", "size": 160, "color": "#ffffff" }
```

### 4.2 Adattamento alla risoluzione
- Fattore di scala unico per tutte le misure. **Modalità `height` (default)**: `H / 1080`. **Modalità `fit`**: `min(H / 1080, W / (1080 × design_aspect))`, per video verticali o più stretti del layout. La modalità si sceglie nel progetto.
- Le posizioni seguono l'ancora (un gruppo in basso a destra resta in basso a destra su 4:3, 16:9, 4K).
- L'editor avvisa se qualche elemento esce dal fotogramma.

### 4.3 Widget

Ogni tipo di widget dichiara uno **schema dei parametri** (tipo, default, intervallo) che genera il pannello proprietà, la validazione e i default dei nuovi widget. Tipi di parametro: scelta da elenco, numero (intervallo/passo), colore RGBA, font+dimensione, booleano, testo/formato, metrica (filtrata per compatibilità), immagine/icona.

Tipi di widget da supportare in v1 (registro di `layout_xml.py` dell'originale):

- **Contenitori**: `composite`/`translate` (gruppo), `frame` (gruppo con sfondo, bordo, raggio, opacità, dissolvenza).
- **Testo e dati**: `text`, `metric`, `metric_unit`, `datetime`, `icon`, `gps_lock_icon`.
- **Mappe**: `moving_map`, `journey_map`, `moving_journey_map`, `circuit_map`, `cairo_circuit_map`.
- **Grafici**: `chart`, `gradient_chart`.
- **Indicatori**: `bar`, `zone_bar`, `compass`, `compass_arrow`, `asi` (air speed), `msi`, `msi2` (motor speed), `cairo_gauge_marker`, `cairo_gauge_round_annotated`, `cairo_gauge_arc_annotated`, `cairo_gauge_donut`.

Nel nostro formato i nomi possono essere razionalizzati (es. eliminare il prefisso `cairo_`); l'importatore mappa i nomi originali.

### 4.4 Metriche e unità

Metriche (dall'originale): `speed`, `cspeed`, `accel`, `gradient`, `cgrad`, `alt`, `odo`, `codo`, `dist`, `azi`, `cog`, `lat`, `lon`, `timestamp`, `gps-dop`, `gps-lock`, `gps-packet`, `gps-packet-index`, `accl.x/y/z`, `grav.x/y/z`, `ori.pitch/roll/yaw`, `hr`, `cadence`, `power`, `temp`, `respiration`, `gear.front`, `gear.rear`, `sdps`.
Le metriche derivate (`cspeed`, `cgrad`, `codo`, `accel`, `azi`, `cog`, …) si calcolano come nell'originale, filtrando i punti con DOP alto. Le metriche non disponibili per una fonte risultano assenti (il widget mostra "--" o l'ultimo valore in grigio, a scelta).

Unità legate alla grandezza fisica:
- velocità: km/h, mph, nodi, m/s, passo (min/km, min/mi, min/nm), spm;
- distanza: km, mi, mn, m;
- altitudine: m, ft;
- temperatura: °C, °F;
- accelerazione: G, m/s².

Ogni widget eredita il sistema di unità del layout e può sovrascriverlo.

### 4.5 Motore di render
- **tiny-skia** (CPU, antialiasing, path): copre anche gli indicatori in stile Cairo.
- Testo con **cosmic-text / rustybuzz** (shaping completo, sostituisce libraqm). Font incorporati (Roboto, Apache-2.0).
- Le parti statiche (sfondi, scale, icone, percorso mappa) si renderizzano una volta e si tengono in cache; per ogni t si ridisegnano solo valori, lancette, marcatori.
- **Icone**: le icone dell'originale vengono da Flaticon e non hanno una licenza libera → **non vengono portate**. Si usa un set con licenza libera (es. Tabler Icons, MIT) con equivalenti semantici (montagna, pendenza, termometro, cuore, contagiri, GPS…); l'importatore mappa i file originali sui nomi semantici.

### 4.6 Importatore dei layout originali
- XML originale → `*.ovl.json`. Risoluzione di riferimento dal nome del file (`default-1920x1080.xml`) o chiesta all'utente.
- Ancoraggio automatico di ogni gruppo di primo livello all'angolo/bordo più vicino; conversione delle stringhe di formato Python e dei nomi di unità; mappatura delle icone.
- Rapporto degli elementi non convertibili (mai scartati in silenzio).
- I 13 layout inclusi nell'originale vengono convertiti una volta e distribuiti con l'app (già ancorati), con avviso di copyright/provenienza.

## 5. Editor

- Selezione dal video o dall'albero dei livelli; riquadri di selezione calcolati da `render` all'istante corrente.
- Spostamento: modifica `offset` (spostare un gruppo sposta i figli). Opzione: aggiornamento automatico dell'ancora in base al quadrante di rilascio.
- Ridimensionamento sul parametro proprio del widget: `size` a proporzioni bloccate (testi, icone, mappe, indicatori) o larghezza/altezza (frame, grafici, barre).
- Selettore d'ancora a 9 punti; linea guida verso il bordo di ancoraggio durante il trascinamento.
- Guide e aggancio, selezione multipla, copia/incolla, annulla/ripeti, palette dei widget.
- Anteprima a più risoluzioni (16:9, 4:3, 9:16, 4K) senza cambiare video.
- Pannello proprietà generato dallo schema (§4.3).

## 6. Progetto, preferenze, integrazione col sistema

### 6.1 File di progetto (facoltativo, `*.ovp.json`)
Video e capitoli, riferimento al layout, GPX/FIT, scarti di sincronizzazione, modalità di scala, zone di privacy, impostazioni di export. Serve solo per spostare/condividere un lavoro.

### 6.2 Preferenze e stato
- **Preferenze globali** nella cartella di configurazione del sistema: ultimo layout usato (applicato ai nuovi video e preselezionato nell'export), ultime impostazioni di export, file recenti, finestra/pannelli, sistema di unità, provider mappe e chiavi API, zone di privacy globali.
- **Stato per video** in un archivio interno dell'app (chiave: identità del file): GPX/FIT collegati, scarto di sincronizzazione, layout scelto, posizione di riproduzione. Nessun file creato accanto ai video.
- Apertura per trascinamento nella finestra o da "Apri con".

### 6.3 Associazione ai formati
- **Windows**: all'avvio, se l'app non è registrata, chiede (con opzione **"Non chiedere più"**) di aggiungersi al menu **"Apri con"** per i formati comuni delle action camera: `.mp4`, `.mov`, `.lrv`, `.insv`. Registrazione solo per l'utente corrente (`HKCU\Software\Classes`, senza privilegi di amministratore) tramite `OpenWithProgids`; non si tenta di diventare l'app predefinita (Windows lo impedisce per programma). Se l'eseguibile è stato spostato, il percorso registrato viene aggiornato in silenzio all'avvio. Nelle preferenze: voce per rimuovere la registrazione.
- **macOS**: tipi di documento dichiarati nell'`Info.plist` del bundle (nessuna richiesta all'utente).
- **Linux**: azione facoltativa "Integra nel sistema" che installa un file `.desktop` con i tipi MIME in `~/.local/share/applications`.

## 7. Export e mappe

**Export**
- Pipeline a stadi in parallelo: decodifica HW → render overlay multi-thread (frame indipendenti) → composizione → codifica.
- **Video finale**: H.264/H.265 con encoder HW (VideoToolbox, NVENC/QSV/AMF, VA-API), fallback x264/x265. Audio copiato. Frame rate e risoluzione dell'originale.
- **Solo overlay**: ProRes 4444 con alpha, oppure sequenza PNG.
- Intervallo esportabile (in/out), avanzamento con tempo stimato, annullamento che lascia un file valido.
- CLI: `actionlay export --layout L --out O VIDEO…` con gli stessi crate.

**Mappe**
- Provider configurabili (OSM e gli altri dell'originale), chiavi API nelle preferenze.
- Cache su disco condivisa; prefetch in background dei tile dell'area del percorso all'apertura del video.
- Rispetto delle regole d'uso OSM (user-agent identificativo, rate limit, niente download massivi) e attribuzione visibile.
- Offline: si usano i tile in cache, riquadri grigi dove mancano, nessun errore bloccante.

## 8. Errori e test

**Errori**: log su file a rotazione (`tracing`) nella cartella dell'app, indicato all'utente in caso di crash. Messaggi non bloccanti per problemi non fatali (HW assente, tile, GPX fuori intervallo). Buchi di telemetria gestiti come in §4.4.

**Test**
- `telemetry`: tracce GPMF reali (in locale `samples/hero7-GX013370.gpmd.bin`, 840 KB, non committata; in CI tracce pubblicabili da reperire) confrontate con i valori prodotti dall'originale; unione GPX/FIT; GPSU; timelapse.
- `layout`: round-trip JSON, validazione schema, importazione di **tutti i 13 layout** senza errori.
- `render`: **immagini di riferimento** generate con l'originale (Python, solo strumento di sviluppo, non distribuito) per ogni widget e layout, confrontate con tolleranza misurata; snapshot propri per le regressioni.
- `media`: spezzoni di 2–3 s tagliati dai campioni reali per seek preciso, passaggio tra capitoli, allineamento A/V.
- End-to-end CLI: export di 3 s, verifica tracce con ffprobe, confronto di frame campione.
- CI GitHub Actions su macOS, Windows x64, Linux x64.

**Campioni grandi**: fuori dal repo (`samples/`, ignorati da git), scaricati da uno script. Il primo campione (`GX013370.MP4`, HERO7) è documentato in `samples/README.md`. Contiene posizioni GPS reali: **non va pubblicato** (decisione del proprietario). Né il video né la telemetria estratta vanno committati; la CI usa solo campioni sintetici o pubblicabili.

## 9. Distribuzione e piattaforme

- **ffmpeg**: release stabile fissata (all'avvio: **n9.0.2**, allineata ai binding `ffmpeg-next`/`ffmpeg-sys-next` 9.0); compilato statico da script nel repo con `--enable-gpl`, **mai `--enable-nonfree`**, solo codec/formati necessari. Compilarlo staticamente in CI su tre piattaforme è il **secondo rischio infrastrutturale** dopo il player.
- Font, icone e layout predefiniti incorporati nel binario.
- **Windows** 10 1809+ x64: un `.exe` con runtime C statico.
- **macOS** 12+: bundle `.app` universale (arm64 + x86_64) in `.dmg`.
- **Linux** x64: un AppImage, baseline glibc 2.31 (Ubuntu 20.04); Vulkan o OpenGL dal sistema.
- Peso atteso: 40–70 MB per piattaforma.
- Rust edition 2024, toolchain stabile fissata con `rust-toolchain.toml`.
- Rilasci su GitHub Releases costruiti dalla CI. **Nessuna firma del codice** per ora (istruzioni nel README per Gatekeeper/SmartScreen); CI predisposta per aggiungerla.
- Licenze: tutto compatibile con GPL-3 (ffmpeg GPL-2.0-or-later, x264/x265 GPL-2.0-or-later, telemetry-parser Apache-2.0, crate MIT/Apache, Roboto Apache-2.0, Tabler MIT). Ogni release include `THIRD_PARTY_LICENSES` (visibile anche in "Informazioni") e l'archivio dei sorgenti di ffmpeg e delle librerie collegate con le opzioni di build. Brevetti H.264/H.265: con encoder/decoder HW il problema è del produttore; x264/x265 software sono distribuiti come fanno VLC/HandBrake/Shotcut.
- Lingua del repo: README, commenti e messaggi di commit in **inglese** (convenzione dei progetti Rust open source); i documenti di design restano in italiano finché il progetto è privato; interfaccia localizzabile (inglese + italiano in v1).

## 10. Fasi interne (ordine di sviluppo)

Ogni fase ha il suo piano di implementazione. Il rilascio pubblico avviene solo al completamento di tutte.

1. **M0 – Prototipo player (riduzione rischio)**: egui + wgpu + ffmpeg statico, decodifica HW, audio, su macOS e Windows, con il campione HERO7 a 100 fps e un 4K H.265 10 bit. Criterio: riproduzione fluida e A/V allineati. Se fallisce: ripiego su **libmpv statico** (GPL, compatibile) per la riproduzione, senza cambiare il resto dell'architettura. Include la build statica di ffmpeg in CI.
2. **M1 – Telemetria**: crate `telemetry` + CLI di dump; confronto con l'originale.
3. **M2 – Layout e render di base**: formato JSON, schema, widget testo/metrica/icona/data/frame, overlay nel player.
4. **M3 – Importatore e tutti i widget**: mappe incluse, immagini di riferimento.
5. **M4 – Editor**.
6. **M5 – Export** (app + CLI).
7. **M6 – Fonti esterne e capitoli**: GPX/FIT, altre camere, capitoli, zone di privacy.
8. **M7 – Rifinitura e distribuzione**: preferenze, "Apri con", pacchetti, licenze, v1.0.

## 11. Credits

- [gopro-dashboard-overlay](https://github.com/time4tea/gopro-dashboard-overlay) di time4tea — ispirazione, know-how su GPMF, metriche e widget; layout originali convertiti.
- [telemetry-parser](https://github.com/AdrianEddy/telemetry-parser), [Gyroflow](https://github.com/gyroflow/gyroflow) (riferimento architetturale), FFmpeg, OpenStreetMap contributors.
