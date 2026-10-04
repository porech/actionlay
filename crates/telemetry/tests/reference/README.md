# Reference values from gopro-dashboard-overlay

Generated once, on 2026-10-05, with gopro-dashboard-overlay **0.134.0**
(`pip install gopro-overlay==0.134.0`, Python 3.14) from the public samples
fetched by `scripts/fetch-gopro-samples.sh`:

    gopro-to-csv.py samples/gopro/<s>.mp4 <s>.gopro-to-csv.csv
    python scripts/reference/gpo-dashboard-reference.py samples/gopro/<s>.mp4 <s>.dashboard.csv

for `<s>` in `hero5`, `hero6`, `max-heromode`.

- `*.gopro-to-csv.csv`: the original's CSV export with its defaults
  (`--gps-dop-max 10`, `--gps-speed-max 60` km/h): one row per GPS sample.
  Used for the raw values (lat, lon, alt, speed, dop, gps_fix, date) and
  the accelerometer.
- `*.dashboard.csv`: the values a dashboard shows, from the processing
  block of `gopro-dashboard.py` replayed verbatim (same defaults). Used for
  the derived metrics, gravity and orientation (orientation in radians).

Known behaviour of the original visible in these files: on max-heromode two
samples, (0, 16) and (4, 0), are missing — their timestamps fall out of
order and the original's `items()` skips them. ActionLay keeps them.

The values are derived from public sample videos (Apache-2.0, GoPro, Inc.).
Never add files produced from private footage here.
