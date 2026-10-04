# Sample videos

Large sample videos live here but are not committed (see `.gitignore`).

| File | Camera | Video | Notes |
|---|---|---|---|
| `GX013370.MP4` | HERO7 (fw HD7.01.01.90.00) | HEVC Main 8-bit, 1920x1440 (4:3), 100 fps, 60 Mbit/s, 3m18s, 1.4 GB | GPS5 18 Hz with 3D fix throughout, DOP 1.3-2.8. Camera clock wrong (creation_time 2016-01-09), GPSU 2026-09-27 12:15:30 UTC. Tracks: hevc, aac, gpmd, fdsc |

The GPMF track of `GX013370.MP4` is kept locally (not committed) as
`samples/hero7-GX013370.gpmd.bin`: it contains the same real GPS track.

Note: sample videos contain real GPS positions (and possibly faces).
`GX013370.MP4` must NOT be published anywhere (owner's decision, 2026-10-04).
Neither the video nor its extracted telemetry may be committed. CI must use
only synthetic or otherwise publishable samples.
