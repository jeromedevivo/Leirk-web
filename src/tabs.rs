//! Tab model: ordering, stacks (Vivaldi), tiling and the unload policy that keeps RAM flat.
use serde::{Deserialize, Serialize};

pub type TabId = u64;

/// Which tab takes over when the active one closes (Vivaldi's `tabs.activation.on_close`).
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum CloseFocus {
    Recent,
    Right,
    Left,
}

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
    /// A stack needs two members; lone members fall back to normal tabs.
    fn unstack_dangling(&mut self) {
        let ids: Vec<_> = self.list.iter().filter_map(|t| t.stack).collect();
        for t in &mut self.list {
            if let Some(s) = t.stack {
                if ids.iter().filter(|&&x| x == s).count() < 2 {
                    t.stack = None;
                }
            }
        }
    }

    /// Opens a tab after `after` (or at the end) and returns its id. Does not activate it.
    pub fn open(&mut self, url: &str, private: bool, after: Option<TabId>) -> TabId {
        self.next_id += 1;
        let id = self.next_id;
        let tab = Tab { id, url: url.into(), private, ..Default::default() };
        match after.and_then(|a| self.index(a)) {
            Some(i) => self.list.insert(i + 1, tab),
            None => self.list.push(tab),
        }
        id
    }

    pub fn activate(&mut self, id: TabId, now: u64) {
        if let Some(t) = self.get_mut(id) {
            t.last_active = now;
            self.active = id;
        }
    }

    /// Closes a tab, activating per `focus`. Keeps at least one tab open.
    pub fn close_with(&mut self, id: TabId, focus: CloseFocus) {
        todo!()
    }

    /// Most recently used tab other than the active one (Ctrl+Tab in recent order).
    pub fn recent_other(&self) -> Option<TabId> {
        todo!()
    }

    /// Closes a tab. Keeps at least one tab open and picks the right neighbour as new active.
    pub fn close(&mut self, id: TabId) {
        let Some(i) = self.index(id) else { return };
        self.list.remove(i);
        self.unstack_dangling();
        self.tiled.retain(|&t| t != id);
        if self.tiled.len() < 2 {
            self.tiled.clear();
        }
        if self.list.is_empty() {
            let n = self.open("", false, None);
            self.active = n;
        } else if self.active == id {
            self.active = self.list[i.min(self.list.len() - 1)].id;
        }
    }

    /// Tabs whose webview is on screen: the tiled set when the active tab is tiled, else the active tab.
    pub fn visible(&self) -> Vec<TabId> {
        if self.tiled.contains(&self.active) { self.tiled.clone() } else { vec![self.active] }
    }

    /// Puts `id` into the stack of `onto` (creating it) and moves it right after its stack.
    pub fn stack(&mut self, id: TabId, onto: TabId) {
        if id == onto || self.get(id).is_none() {
            return;
        }
        let Some(target) = self.get(onto) else { return };
        let sid = target.stack.unwrap_or(onto);
        self.get_mut(onto).unwrap().stack = Some(sid);
        let mut tab = self.list.remove(self.index(id).unwrap());
        tab.stack = Some(sid);
        let last = self.list.iter().rposition(|t| t.stack == Some(sid)).unwrap();
        self.list.insert(last + 1, tab);
        self.unstack_dangling();
    }

    pub fn unstack(&mut self, id: TabId) {
        if let Some(t) = self.get_mut(id) {
            t.stack = None;
        }
        self.unstack_dangling();
    }

    pub fn move_to(&mut self, id: TabId, index: usize) {
        let Some(i) = self.index(id) else { return };
        let tab = self.list.remove(i);
        let index = index.min(self.list.len());
        self.list.insert(index, tab);
    }

    /// Tiles 2..=4 tabs side by side; anything else clears tiling.
    pub fn tile(&mut self, ids: &[TabId]) {
        self.tiled = ids.iter().copied().filter(|&i| self.get(i).is_some()).take(4).collect();
        if self.tiled.len() < 2 {
            self.tiled.clear();
        }
    }

    /// Loaded tabs to unload: idle longer than `idle_secs`, or the least recently used beyond `max_live`.
    /// Visible tabs and those `protect` accepts (audible, exempted sites) are never unloaded.
    pub fn unload_candidates(&self, max_live: usize, idle_secs: u64, now: u64, protect: &dyn Fn(&Tab) -> bool) -> Vec<TabId> {
        let visible = self.visible();
        let mut live: Vec<&Tab> = self.list.iter().filter(|t| t.loaded).collect();
        let keep = |t: &Tab| visible.contains(&t.id) || protect(t);
        // most recent first: everything past max_live (or idle) goes
        live.sort_by(|a, b| b.last_active.cmp(&a.last_active));
        let mut kept = live.iter().filter(|t| keep(t)).count();
        let mut out = vec![];
        for t in live.iter().filter(|t| !keep(t)) {
            let idle = idle_secs > 0 && now.saturating_sub(t.last_active) > idle_secs;
            if idle || kept >= max_live {
                out.push(t.id);
            } else {
                kept += 1;
            }
        }
        out
    }
}

/// Rects (x, y, w, h) for `n` tiled tabs: columns for 2-3, a 2x2 grid for 4.
pub fn tile_rects(n: usize, w: f64, h: f64, gap: f64) -> Vec<(f64, f64, f64, f64)> {
    let (cols, rows) = match n {
        0 | 1 => return vec![(0.0, 0.0, w, h)],
        4 => (2, 2),
        n => (n, 1),
    };
    let cw = (w - gap * (cols - 1) as f64) / cols as f64;
    let rh = (h - gap * (rows - 1) as f64) / rows as f64;
    (0..n).map(|i| (((i % cols) as f64) * (cw + gap), ((i / cols) as f64) * (rh + gap), cw, rh)).collect()
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
        let audible = |t: &Tab| t.audible;
        let mut got = t.unload_candidates(3, 10_000, 1010, &audible);
        got.sort();
        // 6 loaded, max 3 -> unload 3 oldest non-visible non-audible: ids 1,2,3
        assert_eq!(got, vec![ids[1], ids[2], ids[3]]);
        // idle: everything older than 5s except visible/audible
        let mut got = t.unload_candidates(10, 5, 1009, &audible);
        got.sort();
        assert_eq!(got, vec![ids[1], ids[2], ids[3]]);
        // an exempted site is protected like an audible tab
        let site = |t: &Tab| t.audible || t.url.contains("://1.");
        let mut got = t.unload_candidates(3, 10_000, 1010, &site);
        got.sort();
        assert_eq!(got, vec![ids[2], ids[3]]);
    }

    #[test]
    fn close_focus_modes_and_recent_order() {
        let mut t = three();
        let [a, b, c] = [t.list[0].id, t.list[1].id, t.list[2].id];
        t.activate(c, 5);
        t.activate(a, 6);
        t.activate(b, 7);
        assert_eq!(t.recent_other(), Some(a));
        t.close_with(b, CloseFocus::Recent);
        assert_eq!(t.active, a, "most recently used wins");
        t.activate(c, 8);
        t.close_with(c, CloseFocus::Left);
        assert_eq!(t.active, a);
        let mut u = three();
        let first = u.list[0].id;
        u.close_with(first, CloseFocus::Left);
        assert_eq!(u.active, u.list[0].id, "no left neighbour falls back to right");
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
