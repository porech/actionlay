#!/usr/bin/env python3
"""Per-GPS-sample values as gopro-dashboard-overlay shows them on a dashboard.

Development tool only (not distributed): it imports gopro-dashboard-overlay
(GPL-3.0) and replays the processing block of its gopro-dashboard.py, so
ActionLay's derived metrics can be compared with the original's.

Usage: python gpo-dashboard-reference.py VIDEO.mp4 OUT.csv
Needs gopro-dashboard-overlay 0.134.0 and ffmpeg/ffprobe on PATH.
"""
import csv
import sys
from pathlib import Path

from gopro_overlay import gpmd_filters, timeseries_process
from gopro_overlay.ffmpeg import FFMPEG
from gopro_overlay.ffmpeg_gopro import FFMPEGGoPro
from gopro_overlay.framemeta_gpmd import LoadFlag
from gopro_overlay.gpmf import GPS_FIXED_VALUES, GPSFix
from gopro_overlay.loading import GoproLoader
from gopro_overlay.units import units

src, dst = Path(sys.argv[1]), sys.argv[2]
loader = GoproLoader(
    ffmpeg_gopro=FFMPEGGoPro(FFMPEG()),
    units=units,
    flags={LoadFlag.ACCL, LoadFlag.GRAV, LoadFlag.CORI},
    # gopro-dashboard.py defaults: --gps-dop-max 10 --gps-speed-max 60 kph
    gps_lock_filter=gpmd_filters.standard(dop_max=10, speed_max=units.Quantity(60, "kph")),
)
fm = loader.load(src).framemeta
for e in fm.items():
    e.update(raw_lat=e.point.lat, raw_lon=e.point.lon, raw_speed=e.speed)

# Verbatim from gopro-dashboard.py 0.134.0, "processing" block.
packets_per_second = 18
locked_2d = lambda e: e.gpsfix in GPS_FIXED_VALUES
locked_3d = lambda e: e.gpsfix == GPSFix.LOCK_3D.value
fm.process(timeseries_process.process_ses("point", lambda i: i.point, alpha=0.45), filter_fn=locked_2d)
fm.process_deltas(timeseries_process.calculate_speeds(), skip=packets_per_second * 3, filter_fn=locked_2d)
fm.process(timeseries_process.calculate_odo(), filter_fn=locked_2d)
fm.process_accel(timeseries_process.calculate_accel(), skip=18 * 3)
fm.process_deltas(timeseries_process.calculate_gradient(), skip=packets_per_second * 3, filter_fn=locked_3d)
fm.process(timeseries_process.process_kalman("speed", lambda e: e.speed))
fm.process(timeseries_process.filter_locked())


def m(v):
    return "" if v is None else getattr(v, "magnitude", v)


columns = ["packet", "packet_index", "timestamp_ms", "date", "gps_fix", "dop",
           "raw_lat", "raw_lon", "raw_speed", "lat", "lon", "alt", "speed",
           "cspeed", "dist", "codo", "azi", "cog", "cgrad", "accel",
           "accl_x", "accl_y", "accl_z", "grav_x", "grav_y", "grav_z",
           "ori_pitch", "ori_roll", "ori_yaw"]
with open(dst, "w", newline="") as f:
    w = csv.writer(f, lineterminator="\n")
    w.writerow(columns)
    for e in fm.items():
        a, g, o = e.accl, e.grav, e.ori
        w.writerow([
            m(e.packet), m(e.packet_index), m(e.timestamp), e.dt.isoformat(),
            GPSFix(e.gpsfix).name, m(e.dop), e.raw_lat, e.raw_lon, m(e.raw_speed),
            e.point.lat, e.point.lon, m(e.alt), m(e.speed), m(e.cspeed), m(e.dist),
            m(e.codo), m(e.azi), m(e.cog), m(e.cgrad), m(e.accel),
            m(a.x) if a else "", m(a.y) if a else "", m(a.z) if a else "",
            m(g.x) if g else "", m(g.y) if g else "", m(g.z) if g else "",
            m(o.pitch) if o else "", m(o.roll) if o else "", m(o.yaw) if o else "",
        ])
