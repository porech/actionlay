# Public GoPro samples

Downloaded by `scripts/fetch-gopro-samples.sh`; the videos are not committed.

Source: [gopro/gpmf-parser](https://github.com/gopro/gpmf-parser/tree/9a7150632892c7356c91145c889016f07b0ed48d/samples),
commit `9a7150632892c7356c91145c889016f07b0ed48d`.
Copyright GoPro, Inc., licensed under the
[Apache License 2.0](https://www.apache.org/licenses/LICENSE-2.0).
They are used unmodified, as test inputs only.

| File | Camera | Length | GPS | Other streams | Tests |
|---|---|---|---|---|---|
| `hero5.mp4` | HERO5 Black, fw 2.00 | 34.6 s | 3D lock throughout, 618 samples, DOP 4.3–6.1 | ACCL, GYRO, TMPC | reference comparison |
| `hero6.mp4` | HERO6 Black, fw 1.60 | 23.6 s | 3D for 1 s, no lock 1.0–14.0 s, then 2D, then 3D | ACCL, GYRO, TMPC | reference comparison, fix gap |
| `hero7.mp4` | HERO7 Black | 12.7 s | never locks (DOP 99.99) | ACCL, GYRO (no temperature) | no-GPS case |
| `hero8.mp4` | HERO8 Black | 12.7 s | never locks (DOP 99.99) | ACCL, GYRO, TMPC, GRAV, CORI, IORI | no-GPS case, HERO8 streams |
| `max-heromode.mp4` | GoPro MAX, HERO mode | 10.5 s | 3D lock throughout | ACCL, GYRO, TMPC, GRAV, CORI, IORI, MAGN | reference comparison, short final packet |
