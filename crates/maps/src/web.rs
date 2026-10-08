//! Browser tiles: asynchronous fetch with a bounded memory cache, no native threads.
use super::{Settings, Tile};
use std::{
    cell::RefCell,
    collections::{HashMap, HashSet, VecDeque},
    path::PathBuf,
    rc::Rc,
    sync::Arc,
};
use tiny_skia::Pixmap;
use web_time::Instant;

struct State {
    settings: Settings,
    tiles: HashMap<(u64, Tile), Arc<Pixmap>>,
    order: VecDeque<(u64, Tile)>,
    pending: HashSet<(u64, Tile)>,
    failures: HashMap<(u64, Tile), Instant>,
    revision: u64,
    notify: Box<dyn Fn()>,
}
#[derive(Clone)]
pub struct TileStore(Rc<RefCell<State>>);
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
        None
    }
    pub fn new(settings: Settings, _: Option<PathBuf>, notify: impl Fn() + 'static) -> Self {
        Self(Rc::new(RefCell::new(State {
            settings,
            tiles: HashMap::new(),
            order: VecDeque::new(),
            pending: HashSet::new(),
            failures: HashMap::new(),
            revision: 0,
            notify: Box::new(notify),
        })))
    }
    pub fn pending_count(&self) -> usize {
        self.0.borrow().pending.len()
    }
    pub fn settings(&self) -> Settings {
        self.0.borrow().settings.clone()
    }
    pub fn revision(&self) -> u64 {
        self.0.borrow().revision
    }
    pub fn configure(&self, settings: Settings) {
        let mut s = self.0.borrow_mut();
        s.settings = settings;
        s.failures.clear();
        s.revision += 1;
    }
    pub fn get(&self, tile: Tile) -> Option<Arc<Pixmap>> {
        let mut s = self.0.borrow_mut();
        let key = (s.settings.provider(), tile);
        if let Some(image) = s.tiles.get(&key).cloned() {
            s.order.retain(|k| *k != key);
            s.order.push_back(key);
            return Some(image);
        }
        if !s.settings.online
            || !s.settings.valid()
            || s.pending.contains(&key)
            || s.pending.len() >= 4
            || s.failures
                .get(&key)
                .is_some_and(|at| at.elapsed().as_secs() < 300)
        {
            return None;
        }
        let url = s.settings.tile_url(tile);
        s.pending.insert(key);
        let state = self.0.clone();
        wasm_bindgen_futures::spawn_local(async move {
            let image = async {
                let response = reqwest::get(url).await.ok()?.error_for_status().ok()?;
                let bytes = response.bytes().await.ok()?;
                if bytes.len() > 2 * 1024 * 1024 {
                    return None;
                }
                let image = Pixmap::decode_png(&bytes).ok()?;
                (image.width() == 256 && image.height() == 256).then_some(image)
            }
            .await;
            let mut s = state.borrow_mut();
            s.pending.remove(&key);
            if let Some(image) = image {
                put(&mut s, key, image);
            } else {
                s.failures.insert(key, Instant::now());
            }
            s.revision += 1;
            (s.notify)();
        });
        None
    }
    pub fn insert(&self, tile: Tile, image: Pixmap) {
        let mut s = self.0.borrow_mut();
        let key = (s.settings.provider(), tile);
        put(&mut s, key, image);
        s.revision += 1;
    }
}
fn put(s: &mut State, key: (u64, Tile), image: Pixmap) {
    s.order.retain(|k| *k != key);
    s.order.push_back(key);
    s.tiles.insert(key, Arc::new(image));
    while s.order.len() > 128 {
        if let Some(old) = s.order.pop_front() {
            s.tiles.remove(&old);
        }
    }
}
