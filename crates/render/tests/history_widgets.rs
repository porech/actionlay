use actionlay_layout::Layout;
use actionlay_maps::{Settings, TileStore};
use actionlay_render::{Renderer, tiny_skia::Pixmap};
use actionlay_telemetry::{GpsLock, Metric, Snapshot, Telemetry, Value};

fn layout(nodes: &str) -> Layout {
    Layout::from_json(&format!(r#"{{"version":1,"nodes":{nodes}}}"#))
        .unwrap()
        .layout
}
fn render(r: &mut Renderer, l: &Layout, t: &Telemetry, at: f64) -> Pixmap {
    let mut p = Pixmap::new(960, 540).unwrap();
    r.render_telemetry_into(l, t, at, &mut p);
    p
}
#[test]
fn history_widgets_are_deterministic_when_seeking_backwards() {
    let l = layout(
        r#"[
      {"type":"chart","metric":"alt","size":[500,180],"seconds":5},
      {"type":"map","mode":"circuit","size":[400,260],"offset":[0,200]},
      {"type":"g_meter","show_peaks":true,"offset":[550,200],"diameter":240}
    ]"#,
    );
    let tel = Telemetry::for_test(
        10.0,
        &[
            (Metric::Alt, vec![(0.0, 10.0), (5.0, 15.0), (9.0, 12.0)]),
            (
                Metric::Lat,
                vec![(0.0, 45.0), (1.0, 45.001), (2.0, 45.002), (3.0, 45.003)],
            ),
            (
                Metric::Lon,
                vec![(0.0, 9.0), (1.0, 9.001), (2.0, 9.002), (3.0, 9.003)],
            ),
            (
                Metric::AccelLon,
                vec![(0.0, 0.0), (1.0, 1.0), (2.0, 2.0), (3.0, -3.0)],
            ),
            (
                Metric::AccelLat,
                vec![(0.0, 0.0), (1.0, 2.0), (2.0, -1.0), (3.0, 3.0)],
            ),
        ],
    );
    let mut r = Renderer::new();
    let at_two = render(&mut r, &l, &tel, 2.0);
    assert_ne!(at_two.data(), render(&mut r, &l, &tel, 3.0).data());
    assert_eq!(at_two.data(), render(&mut r, &l, &tel, 2.0).data());
}
#[test]
fn g_meter_does_not_freeze_a_stale_value_forever() {
    let l = layout(r#"[{"type":"g_meter","stale_secs":2,"diameter":240}]"#);
    let mut r = Renderer::new();
    let stale = |age| {
        Snapshot::for_test(
            10.0,
            None,
            GpsLock::NoLock,
            &[
                (Metric::AccelLon, Value::Stale { value: 4.0, age }),
                (Metric::AccelLat, Value::Stale { value: 3.0, age }),
            ],
        )
    };
    let grace = r.render(&l, &stale(1.0), 960, 540);
    let expired = r.render(&l, &stale(3.0), 960, 540);
    assert_ne!(grace.data(), expired.data());
    let empty = Snapshot::for_test(
        10.0,
        None,
        GpsLock::NoLock,
        &[
            (
                Metric::AccelLon,
                Value::Stale {
                    value: 0.0,
                    age: 100.0,
                },
            ),
            (
                Metric::AccelLat,
                Value::Stale {
                    value: 0.0,
                    age: 100.0,
                },
            ),
        ],
    );
    assert_eq!(expired.data(), r.render(&l, &empty, 960, 540).data());
}
#[test]
fn privacy_zone_prevents_requests_for_a_hidden_current_position() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let store = TileStore::new(
        Settings {
            url: format!(
                "http://{}/{{z}}/{{x}}/{{y}}.png",
                listener.local_addr().unwrap()
            ),
            privacy: vec![actionlay_maps::PrivacyZone {
                lat: 45.0,
                lon: 9.0,
                radius_m: 250.0,
            }],
            ..Default::default()
        },
        None,
        || {},
    );
    let mut r = Renderer::new();
    r.set_maps(store);
    let l = layout(r#"[{"type":"map","size":[300,240]}]"#);
    let snap = Snapshot::for_test(
        0.0,
        None,
        GpsLock::Lock3d,
        &[
            (Metric::Lat, Value::Present(45.0)),
            (Metric::Lon, Value::Present(9.0)),
        ],
    );
    r.render(&l, &snap, 960, 540);
    std::thread::sleep(std::time::Duration::from_millis(100));
    assert!(matches!(listener.accept(),Err(e) if e.kind()==std::io::ErrorKind::WouldBlock));
}

#[test]
fn map_route_modes_split_colors_rotation_and_marker_size() {
    let tel = Telemetry::for_test(
        3.0,
        &[
            (Metric::Lat, vec![(0.0, 45.0), (1.0, 45.0), (2.0, 45.0)]),
            (Metric::Lon, vec![(0.0, 9.0), (1.0, 9.001), (2.0, 9.002)]),
            (Metric::Cog, vec![(0.0, 90.0), (2.0, 90.0)]),
        ],
    );
    let mut r = Renderer::new();
    r.set_maps(TileStore::new(
        Settings {
            online: false,
            ..Default::default()
        },
        None,
        || {},
    ));
    let make = |options: &str| {
        layout(&format!(
            r##"[{{"type":"map","size":[400,300],"mode":"circuit","background":"#000000","show_marker":false,{options}}}]"##
        ))
    };
    let count = |p: &Pixmap, rgb: (u8, u8, u8)| {
        p.pixels()
            .iter()
            .filter(|c| (c.red(), c.green(), c.blue()) == rgb)
            .count()
    };
    let green = (22, 101, 52);
    let yellow = (250, 204, 21);
    let full = make(r#""route_mode":"full","route_width":8"#);
    let at_start = render(&mut r, &full, &tel, 0.0);
    assert_eq!(count(&at_start, green), 0);
    assert!(count(&at_start, yellow) > 100);
    let middle = render(&mut r, &full, &tel, 1.0);
    assert!(count(&middle, green) > 100 && count(&middle, yellow) > 100);
    let end = render(&mut r, &full, &tel, 2.0);
    assert!(count(&end, green) > 100);
    assert_eq!(count(&end, yellow), 0);
    assert_eq!(middle.data(), render(&mut r, &full, &tel, 1.0).data());
    let none = render(&mut r, &make(r#""orientation":"north_up""#), &tel, 1.0);
    assert_eq!(count(&none, green) + count(&none, yellow), 0);
    let past = render(
        &mut r,
        &make(r#""route_mode":"past","route_width":8"#),
        &tel,
        1.0,
    );
    assert!(count(&past, green) > 100);
    assert_eq!(count(&past, yellow), 0);
    let rotated = render(
        &mut r,
        &make(r#""route_mode":"full","route_width":8,"orientation":"course_up""#),
        &tel,
        1.0,
    );
    let coords: Vec<_> = rotated
        .pixels()
        .iter()
        .enumerate()
        .filter(|(_, c)| (c.red(), c.green(), c.blue()) == yellow)
        .map(|(i, _)| (i % 960, i / 960))
        .collect();
    let width =
        coords.iter().map(|c| c.0).max().unwrap() - coords.iter().map(|c| c.0).min().unwrap();
    let height =
        coords.iter().map(|c| c.1).max().unwrap() - coords.iter().map(|c| c.1).min().unwrap();
    assert!(height > width * 3, "eastbound route must point up");
    let dot = |radius| {
        layout(&format!(
            r##"[{{"type":"map","size":[400,300],"mode":"circuit","background":"#000000","marker":"#ff0000","marker_radius":{radius}}}]"##
        ))
    };
    assert!(
        count(&render(&mut r, &dot(12), &tel, 1.0), (255, 0, 0))
            > count(&render(&mut r, &dot(3), &tel, 1.0), (255, 0, 0)) * 4
    );
}

#[test]
fn missing_gps_and_units_respect_each_widgets_tolerance() {
    for node in [
        r#"{"type":"metric","metric":"speed","stale_secs":3}"#,
        r#"{"type":"gps_lock_icon","stale_secs":3}"#,
        r#"{"type":"map","size":[300,220],"stale_secs":3}"#,
    ] {
        let l = layout(&format!("[{node}]"));
        let snapshot = |age| {
            Snapshot::for_test(
                10.0,
                None,
                GpsLock::Unknown,
                &[
                    (Metric::Speed, Value::Stale { value: 12.0, age }),
                    (Metric::GpsLock, Value::Stale { value: 3.0, age }),
                    (Metric::Lat, Value::Stale { value: 45.0, age }),
                    (Metric::Lon, Value::Stale { value: 9.0, age }),
                ],
            )
        };
        let mut r = Renderer::new();
        let within = r.render(&l, &snapshot(3.0), 960, 540);
        let expired = r.render(&l, &snapshot(3.01), 960, 540);
        assert_ne!(within.data(), expired.data(), "{node}");
        let immediate = layout(&format!(
            "[{}]",
            node.replace("\"stale_secs\":3", "\"stale_secs\":0")
        ));
        assert_eq!(
            expired.data(),
            r.render(&immediate, &snapshot(0.01), 960, 540).data(),
            "{node}"
        );
    }
}
