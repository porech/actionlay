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
