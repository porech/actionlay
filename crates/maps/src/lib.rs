//! Nonblocking raster tiles. Only requested visible tiles are fetched; no route prefetch.
use std::collections::{HashMap, HashSet, VecDeque};
use std::hash::{Hash, Hasher};
use std::io::Read;
use std::path::PathBuf;
use std::sync::{
    Arc, Condvar, Mutex,
    atomic::{AtomicU64, Ordering},
};
use std::time::{Duration, Instant, SystemTime};

use serde::{Deserialize, Serialize};
use tiny_skia::Pixmap;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PrivacyZone {
    pub lat: f64,
    pub lon: f64,
    pub radius_m: f64,
}

impl PrivacyZone {
    pub fn valid(&self) -> bool {
        self.lat.is_finite()
            && (-85.0..=85.0).contains(&self.lat)
            && self.lon.is_finite()
            && (-180.0..=180.0).contains(&self.lon)
            && self.radius_m.is_finite()
            && self.radius_m > 0.0
    }
    pub fn contains(&self, lat: f64, lon: f64) -> bool {
        self.crosses((lat, lon), (lat, lon))
    }
    pub fn crosses(&self, a: (f64, f64), b: (f64, f64)) -> bool {
        if !self.valid() {
            return false;
        }
        let project = |(lat, lon): (f64, f64)| {
            [
                ((lon - self.lon + 180.0).rem_euclid(360.0) - 180.0)
                    * 111_195.0
                    * self.lat.to_radians().cos(),
                (lat - self.lat) * 111_195.0,
            ]
        };
        let a = project(a);
        let b = project(b);
        let d = [b[0] - a[0], b[1] - a[1]];
        let len = d[0] * d[0] + d[1] * d[1];
        let u = if len == 0.0 {
            0.0
        } else {
            (-(a[0] * d[0] + a[1] * d[1]) / len).clamp(0.0, 1.0)
        };
        (a[0] + u * d[0]).hypot(a[1] + u * d[1]) <= self.radius_m
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub online: bool,
    pub url: String,
    pub api_key: String,
    pub attribution: String,
    pub privacy: Vec<PrivacyZone>,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            online: true,
            url: "https://tile.openstreetmap.org/{z}/{x}/{y}.png".into(),
            api_key: String::new(),
            attribution: "© OpenStreetMap contributors".into(),
            privacy: Vec::new(),
        }
    }
}
impl Settings {
    pub fn valid(&self) -> bool {
        reqwest::Url::parse(&self.url).is_ok_and(|u| matches!(u.scheme(), "https" | "http"))
            && ["{z}", "{x}", "{y}"].iter().all(|p| self.url.contains(p))
            && !self.attribution.trim().is_empty()
            && self.privacy.iter().all(PrivacyZone::valid)
    }
    fn provider(&self) -> u64 {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        self.url.hash(&mut h);
        self.api_key.hash(&mut h);
        h.finish()
    }
    fn tile_url(&self, key: Tile) -> String {
        let api_key: String = self
            .api_key
            .bytes()
            .map(|b| {
                if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
                    (b as char).to_string()
                } else {
                    format!("%{b:02X}")
                }
            })
            .collect();
        self.url
            .replace("{z}", &key.z.to_string())
            .replace("{x}", &key.x.to_string())
            .replace("{y}", &key.y.to_string())
            .replace("{api_key}", &api_key)
    }
}

/// Normalised Web Mercator coordinates. X is periodic; Y is clamped at the poles.
pub fn project(lat: f64, lon: f64) -> [f64; 2] {
    let lat = lat.clamp(-85.05112878, 85.05112878).to_radians();
    [
        (lon + 180.0) / 360.0,
        (1.0 - (lat.tan() + 1.0 / lat.cos()).ln() / std::f64::consts::PI) / 2.0,
    ]
}
pub fn wrap_delta(delta: f64) -> f64 {
    (delta + 0.5).rem_euclid(1.0) - 0.5
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Tile {
    pub z: u8,
    pub x: u32,
    pub y: u32,
}
impl Tile {
    pub fn at(z: u8, x: i64, y: i64) -> Option<Self> {
        let n = 1_i64 << z.min(19);
        (y >= 0 && y < n).then_some(Self {
            z: z.min(19),
            x: x.rem_euclid(n) as u32,
            y: y as u32,
        })
    }
}
#[derive(Clone, Copy, Hash, PartialEq, Eq)]
struct Key {
    provider: u64,
    tile: Tile,
}
struct State {
    settings: Settings,
    tiles: HashMap<Key, Arc<Pixmap>>,
    order: VecDeque<Key>,
    pending: HashSet<Key>,
    jobs: VecDeque<(Key, Settings)>,
    failures: HashMap<Key, Instant>,
    quit: bool,
}
struct Shared {
    state: Mutex<State>,
    wake: Condvar,
    revision: AtomicU64,
}
struct Handle {
    shared: Arc<Shared>,
    worker: bool,
}
impl Drop for Handle {
    fn drop(&mut self) {
        self.shared.state.lock().unwrap().quit = true;
        self.shared.wake.notify_all();
    }
}
#[derive(Clone)]
pub struct TileStore {
    handle: Arc<Handle>,
}

impl TileStore {
    pub fn offline() -> Self {
        Self::new(
            Settings {
                online: false,
                ..Default::default()
            },
            None,
            || {},
        )
    }
    pub fn default_cache_dir() -> Option<PathBuf> {
        directories::ProjectDirs::from("org", "ActionLay", "ActionLay")
            .map(|d| d.cache_dir().join("tiles"))
    }
    pub fn new(
        settings: Settings,
        directory: Option<PathBuf>,
        notify: impl Fn() + Send + 'static,
    ) -> Self {
        let worker_enabled = settings.online || directory.is_some();
        let shared = Arc::new(Shared {
            state: Mutex::new(State {
                settings,
                tiles: HashMap::new(),
                order: VecDeque::new(),
                pending: HashSet::new(),
                jobs: VecDeque::new(),
                failures: HashMap::new(),
                quit: false,
            }),
            wake: Condvar::new(),
            revision: AtomicU64::new(0),
        });
        if worker_enabled {
            let worker = shared.clone();
            std::thread::Builder::new()
                .name("map-tiles".into())
                .spawn(move || run(worker, directory, notify))
                .expect("map worker");
        }
        Self {
            handle: Arc::new(Handle {
                shared,
                worker: worker_enabled,
            }),
        }
    }
    pub fn settings(&self) -> Settings {
        self.handle.shared.state.lock().unwrap().settings.clone()
    }
    pub fn revision(&self) -> u64 {
        self.handle.shared.revision.load(Ordering::Relaxed)
    }
    pub fn configure(&self, settings: Settings) {
        let mut s = self.handle.shared.state.lock().unwrap();
        s.settings = settings;
        s.jobs.clear();
        s.pending.clear();
        s.failures.clear();
        self.handle.shared.revision.fetch_add(1, Ordering::Relaxed);
    }
    pub fn get(&self, tile: Tile) -> Option<Arc<Pixmap>> {
        let shared = &self.handle.shared;
        let mut s = shared.state.lock().unwrap();
        let key = Key {
            provider: s.settings.provider(),
            tile,
        };
        if let Some(p) = s.tiles.get(&key).cloned() {
            s.order.retain(|k| *k != key);
            s.order.push_back(key);
            return Some(p);
        }
        if !self.handle.worker {
            return None;
        }
        if s.pending.contains(&key)
            || s.failures
                .get(&key)
                .is_some_and(|t| t.elapsed() < Duration::from_secs(300))
        {
            return None;
        }
        if s.jobs.len() >= 32
            && let Some((old, _)) = s.jobs.pop_front()
        {
            s.pending.remove(&old);
        }
        let settings = s.settings.clone();
        s.pending.insert(key);
        s.jobs.push_back((key, settings));
        shared.wake.notify_one();
        None
    }
    /// For deterministic offline rendering and tests; no network request is involved.
    pub fn insert(&self, tile: Tile, pixmap: Pixmap) {
        let mut s = self.handle.shared.state.lock().unwrap();
        let key = Key {
            provider: s.settings.provider(),
            tile,
        };
        put(&mut s, key, pixmap);
        self.handle.shared.revision.fetch_add(1, Ordering::Relaxed);
    }
}
fn put(s: &mut State, key: Key, pixmap: Pixmap) {
    s.order.retain(|k| *k != key);
    s.order.push_back(key);
    s.tiles.insert(key, Arc::new(pixmap));
    while s.order.len() > 128 {
        if let Some(old) = s.order.pop_front() {
            s.tiles.remove(&old);
        }
    }
}
fn run(shared: Arc<Shared>, directory: Option<PathBuf>, notify: impl Fn()) {
    let client = reqwest::blocking::Client::builder()
        .user_agent("ActionLay/0.1 (+https://github.com/porech/actionlay)")
        .timeout(Duration::from_secs(8))
        .build()
        .ok();
    let mut last_fetch = Instant::now() - Duration::from_secs(1);
    let mut backoff = HashMap::<u64, Instant>::new();
    loop {
        let (key, settings) = {
            let mut s = shared.state.lock().unwrap();
            loop {
                if s.quit {
                    return;
                }
                if let Some(job) = s.jobs.pop_back() {
                    break job;
                }
                s = shared.wake.wait(s).unwrap();
            }
        };
        let path = directory.as_ref().map(|d| {
            d.join(format!(
                "{:016x}/{}-{}-{}.png",
                key.provider, key.tile.z, key.tile.x, key.tile.y
            ))
        });
        let cached = path
            .as_ref()
            .and_then(|p| std::fs::read(p).ok())
            .and_then(|b| Pixmap::decode_png(&b).ok())
            .filter(|p| p.width() == 256 && p.height() == 256);
        let fresh = cached.is_some()
            && path
                .as_ref()
                .and_then(|p| std::fs::metadata(p).ok())
                .and_then(|m| m.modified().ok())
                .is_some_and(|t| {
                    SystemTime::now().duration_since(t).unwrap_or_default()
                        < Duration::from_secs(7 * 86400)
                });
        if let Some(p) = cached {
            let mut s = shared.state.lock().unwrap();
            put(&mut s, key, p);
            shared.revision.fetch_add(1, Ordering::Relaxed);
            drop(s);
            notify();
        }
        let may_fetch = settings.online
            && settings.valid()
            && !fresh
            && backoff
                .get(&key.provider)
                .is_none_or(|until| Instant::now() >= *until);
        let fetched = if may_fetch {
            // One connection and at most four requests/second. Never log URLs/API keys.
            let wait = Duration::from_millis(250).saturating_sub(last_fetch.elapsed());
            std::thread::sleep(wait);
            {
                let mut state = shared.state.lock().unwrap();
                if state.quit {
                    return;
                }
                if state.settings.provider() != key.provider || !state.settings.online {
                    state.pending.remove(&key);
                    continue;
                }
            }
            last_fetch = Instant::now();
            client
                .as_ref()
                .and_then(|c| c.get(settings.tile_url(key.tile)).send().ok())
                .filter(|r| {
                    if matches!(r.status().as_u16(), 429 | 503) {
                        let seconds = r
                            .headers()
                            .get("retry-after")
                            .and_then(|v| v.to_str().ok())
                            .and_then(|s| s.parse::<u64>().ok())
                            .unwrap_or(60)
                            .clamp(1, 86400);
                        backoff.insert(key.provider, Instant::now() + Duration::from_secs(seconds));
                    }
                    r.status().is_success()
                })
                .and_then(|r| {
                    let mut b = Vec::new();
                    r.take(2 * 1024 * 1024).read_to_end(&mut b).ok()?;
                    let p = Pixmap::decode_png(&b).ok()?;
                    (p.width() == 256 && p.height() == 256).then_some((b, p))
                })
        } else {
            None
        };
        let mut s = shared.state.lock().unwrap();
        s.pending.remove(&key);
        if let Some((bytes, p)) = fetched {
            put(&mut s, key, p);
            shared.revision.fetch_add(1, Ordering::Relaxed);
            drop(s);
            if let Some(path) = path {
                if let Some(parent) = path.parent() {
                    std::fs::create_dir_all(parent).ok();
                }
                let tmp = path.with_extension("tmp");
                if std::fs::write(&tmp, bytes).is_ok() {
                    std::fs::rename(tmp, path).ok();
                }
            }
            notify();
        } else {
            s.failures.insert(key, Instant::now());
            if s.failures.len() > 512 {
                s.failures.clear();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn wait_tile(store: &TileStore, key: Tile) -> Arc<Pixmap> {
        let start = Instant::now();
        loop {
            if let Some(p) = store.get(key) {
                return p;
            }
            assert!(
                start.elapsed() < Duration::from_secs(5),
                "tile not delivered"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn corrupt_cache_is_refetched_and_fresh_disk_cache_works_offline() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        listener.set_nonblocking(true).unwrap();
        let mut image = Pixmap::new(256, 256).unwrap();
        image.fill(tiny_skia::Color::from_rgba8(35, 80, 120, 255));
        let bytes = image.encode_png().unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        let server = std::thread::spawn(move || {
            let start = Instant::now();
            let mut stream = loop {
                if let Ok((s, _)) = listener.accept() {
                    break s;
                }
                assert!(start.elapsed() < Duration::from_secs(5));
                std::thread::sleep(Duration::from_millis(5));
            };
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut request = Vec::new();
            while !request.ends_with(b"\r\n\r\n") {
                let mut b = [0];
                stream.read_exact(&mut b).unwrap();
                request.push(b[0]);
            }
            tx.send(String::from_utf8(request).unwrap()).unwrap();
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                bytes.len()
            )
            .unwrap();
            stream.write_all(&bytes).unwrap();
        });
        let settings = Settings {
            url: format!("http://{address}/{{z}}/{{x}}/{{y}}.png"),
            ..Default::default()
        };
        let directory = std::env::temp_dir().join(format!(
            "actionlay-tiles-{}-{}",
            std::process::id(),
            address.port()
        ));
        let key = Tile { z: 1, x: 0, y: 0 };
        let path = directory.join(format!("{:016x}/1-0-0.png", settings.provider()));
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, b"invalid PNG despite fresh mtime").unwrap();
        let store = TileStore::new(settings.clone(), Some(directory.clone()), || {});
        for _ in 0..20 {
            store.get(key);
        }
        assert_eq!(wait_tile(&store, key).data(), image.data());
        let request = rx.recv_timeout(Duration::from_secs(3)).unwrap();
        assert!(request.starts_with("GET /1/0/0.png"));
        assert!(
            request
                .to_ascii_lowercase()
                .contains("user-agent: actionlay/")
        );
        server.join().unwrap();
        let start = Instant::now();
        while Pixmap::load_png(&path).is_err() {
            assert!(start.elapsed() < Duration::from_secs(2));
            std::thread::sleep(Duration::from_millis(5));
        }
        drop(store);
        let offline = TileStore::new(
            Settings {
                online: false,
                ..settings
            },
            Some(directory.clone()),
            || {},
        );
        assert_eq!(wait_tile(&offline, key).data(), image.data());
        drop(offline);
        std::fs::remove_dir_all(directory).ok();
    }

    #[test]
    fn offline_memory_store_never_creates_network_jobs() {
        let s = TileStore::offline();
        assert!(s.get(Tile { z: 1, x: 0, y: 0 }).is_none());
        assert!(s.handle.shared.state.lock().unwrap().jobs.is_empty());
    }
    #[test]
    fn mercator_wrap_and_privacy_crossings() {
        assert!((project(0.0, 0.0)[0] - 0.5).abs() < 1e-10);
        assert!(
            (wrap_delta(project(0.0, -179.0)[0] - project(0.0, 179.0)[0]) - 2.0 / 360.0).abs()
                < 1e-10
        );
        assert_eq!(Tile::at(2, -1, 0), Some(Tile { z: 2, x: 3, y: 0 }));
        assert!(Tile::at(2, 0, -1).is_none());
        let z = PrivacyZone {
            lat: 0.0,
            lon: 0.0,
            radius_m: 100.0,
        };
        assert!(z.contains(0.0, 0.0));
        assert!(z.crosses((0.0, -0.01), (0.0, 0.01)));
        assert!(!z.crosses((0.01, -0.01), (0.01, 0.01)));
    }
    #[test]
    fn cache_is_bounded_and_settings_keep_provider_tiles_separate() {
        let s = TileStore::offline();
        let k = Tile { z: 1, x: 0, y: 0 };
        s.insert(k, Pixmap::new(256, 256).unwrap());
        assert!(s.get(k).is_some());
        s.configure(Settings {
            url: "https://example.org/{z}/{x}/{y}.png".into(),
            online: false,
            ..Default::default()
        });
        assert!(s.get(k).is_none());
        for x in 0..140 {
            s.insert(Tile { z: 8, x, y: 0 }, Pixmap::new(256, 256).unwrap());
        }
        assert!(s.handle.shared.state.lock().unwrap().tiles.len() <= 128);
    }
}
