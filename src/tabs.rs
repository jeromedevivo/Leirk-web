//! Tab model: ordering, stacks (Vivaldi), tiling and the unload policy that keeps RAM flat.
use serde::{Deserialize, Serialize};

pub type TabId = u64;

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Tab {
    pub id: TabId,
    pub url: String,
    pub title: String,
    #[serde(default)]
    pub favicon: String,
    /// Tabs sharing a stack id are shown as one Vivaldi-style stack.
    #[serde(default)]
    pub stack: Option<TabId>,
    #[serde(default)]
    pub pinned: bool,
    #[serde(skip)]
    pub private: bool,
    /// A live WebView2 exists for this tab.
    #[serde(skip)]
    pub loaded: bool,
    #[serde(skip)]
    pub audible: bool,
    #[serde(skip)]
    pub last_active: u64,
}

#[derive(Serialize, Deserialize, Default, Debug)]
pub struct Tabs {
    pub list: Vec<Tab>,
    pub active: TabId,
    #[serde(default)]
    pub tiled: Vec<TabId>,
    #[serde(default)]
    next_id: TabId,
}

impl Tabs {
    pub fn get(&self, id: TabId) -> Option<&Tab> {
        self.list.iter().find(|t| t.id == id)
    }
    pub fn get_mut(&mut self, id: TabId) -> Option<&mut Tab> {
        self.list.iter_mut().find(|t| t.id == id)
    }
    fn index(&self, id: TabId) -> Option<usize> {
        self.list.iter().position(|t| t.id == id)
    }

    /// Opens a tab after `after` (or at the end) and returns its id. Does not activate it.
    pub fn open(&mut self, url: &str, private: bool, after: Option<TabId>) -> TabId {
        todo!()
    }

    pub fn activate(&mut self, id: TabId, now: u64) {
        todo!()
    }

    /// Closes a tab. Keeps at least one tab open and picks the right neighbour as new active.
    pub fn close(&mut self, id: TabId) {
        todo!()
    }

    /// Tabs whose webview is on screen: the tiled set when the active tab is tiled, else the active tab.
    pub fn visible(&self) -> Vec<TabId> {
        todo!()
    }

    /// Puts `id` into the stack of `onto` (creating it) and moves it right after its stack.
    pub fn stack(&mut self, id: TabId, onto: TabId) {
        todo!()
    }

    pub fn unstack(&mut self, id: TabId) {
        todo!()
    }

    pub fn move_to(&mut self, id: TabId, index: usize) {
        todo!()
    }

    /// Tiles 2..=4 tabs side by side; anything else clears tiling.
    pub fn tile(&mut self, ids: &[TabId]) {
        todo!()
    }

    /// Loaded tabs to unload: idle longer than `idle_secs`, or the least recently used beyond `max_live`.
    /// Visible and audible tabs are never unloaded.
    pub fn unload_candidates(&self, max_live: usize, idle_secs: u64, now: u64) -> Vec<TabId> {
        todo!()
    }
}

/// Rects (x, y, w, h) for `n` tiled tabs: columns for 2-3, a 2x2 grid for 4.
pub fn tile_rects(n: usize, w: f64, h: f64, gap: f64) -> Vec<(f64, f64, f64, f64)> {
    todo!()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn three() -> Tabs {
        let mut t = Tabs::default();
        let a = t.open("https://a.com", false, None);
        t.open("https://b.com", false, None);
        t.open("https://c.com", false, None);
        t.activate(a, 0);
        t
    }
    fn ids(t: &Tabs) -> Vec<TabId> {
        t.list.iter().map(|t| t.id).collect()
    }

    #[test]
    fn open_appends_or_inserts_after() {
        let mut t = three();
        let first = t.list[0].id;
        let n = t.open("https://d.com", false, Some(first));
        assert_eq!(t.list[1].id, n);
        assert_eq!(t.list.len(), 4);
        let mut all = ids(&t);
        all.dedup();
        all.sort();
        all.dedup();
        assert_eq!(all.len(), 4, "ids are unique");
    }

    #[test]
    fn close_active_picks_right_then_left() {
        let mut t = three();
        let [a, b, c] = [t.list[0].id, t.list[1].id, t.list[2].id];
        t.close(a);
        assert_eq!(t.active, b);
        t.activate(c, 1);
        t.close(c);
        assert_eq!(t.active, b);
    }

    #[test]
    fn closing_last_tab_leaves_a_blank_one() {
        let mut t = Tabs::default();
        let a = t.open("https://a.com", false, None);
        t.activate(a, 0);
        t.close(a);
        assert_eq!(t.list.len(), 1);
        assert_eq!(t.list[0].url, "");
        assert_eq!(t.active, t.list[0].id);
    }

    #[test]
    fn stack_groups_and_moves_next_to_target() {
        let mut t = three();
        let [a, b, c] = [t.list[0].id, t.list[1].id, t.list[2].id];
        t.stack(c, a);
        assert_eq!(ids(&t), vec![a, c, b]);
        assert_eq!(t.get(a).unwrap().stack, t.get(c).unwrap().stack);
        assert!(t.get(a).unwrap().stack.is_some());
        t.unstack(c);
        assert_eq!(t.get(c).unwrap().stack, None);
        // a stack of one is dissolved
        assert_eq!(t.get(a).unwrap().stack, None);
    }

    #[test]
    fn tiling_shows_tiled_set_only_when_active_is_in_it() {
        let mut t = three();
        let [a, b, c] = [t.list[0].id, t.list[1].id, t.list[2].id];
        t.tile(&[a, b]);
        assert_eq!(t.visible(), vec![a, b]);
        t.activate(c, 1);
        assert_eq!(t.visible(), vec![c]);
        t.tile(&[a]);
        assert!(t.tiled.is_empty());
        t.tile(&[a, b]);
        t.close(b);
        assert!(t.tiled.is_empty(), "a tile set of one is dissolved");
    }

    #[test]
    fn unload_policy_respects_idle_lru_visible_and_audio() {
        let mut t = Tabs::default();
        let ids: Vec<_> = (0..6).map(|i| t.open(&format!("https://{i}.com"), false, None)).collect();
        for (i, &id) in ids.iter().enumerate() {
            t.activate(id, 1000 + i as u64);
            t.get_mut(id).unwrap().loaded = true;
        }
        // active = ids[5]; ids[0] is audible
        t.get_mut(ids[0]).unwrap().audible = true;
        let mut got = t.unload_candidates(3, 10_000, 1010);
        got.sort();
        // 6 loaded, max 3 -> unload 3 oldest non-visible non-audible: ids 1,2,3
        assert_eq!(got, vec![ids[1], ids[2], ids[3]]);
        // idle: everything older than 5s except visible/audible
        let mut got = t.unload_candidates(10, 5, 1009);
        got.sort();
        assert_eq!(got, vec![ids[1], ids[2], ids[3]]);
    }

    #[test]
    fn move_to_reorders() {
        let mut t = three();
        let [a, b, c] = [t.list[0].id, t.list[1].id, t.list[2].id];
        t.move_to(a, 2);
        assert_eq!(ids(&t), vec![b, c, a]);
        t.move_to(a, 99);
        assert_eq!(ids(&t), vec![b, c, a]);
    }

    #[test]
    fn tile_rects_layouts() {
        assert_eq!(tile_rects(1, 100.0, 50.0, 0.0), vec![(0.0, 0.0, 100.0, 50.0)]);
        assert_eq!(tile_rects(2, 100.0, 50.0, 0.0), vec![(0.0, 0.0, 50.0, 50.0), (50.0, 0.0, 50.0, 50.0)]);
        let four = tile_rects(4, 100.0, 100.0, 0.0);
        assert_eq!(four[3], (50.0, 50.0, 50.0, 50.0));
        let gap = tile_rects(2, 104.0, 50.0, 4.0);
        assert_eq!(gap[1].0, 54.0);
        assert_eq!(gap[0].2, 50.0);
    }
}
