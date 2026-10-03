//! The search engine: a flat list of items, fuzzy-matched on every keystroke.

use crate::usage::Usage;
use crate::Item;
use nucleo_matcher::pattern::{AtomKind, CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher, Utf32String};
use serde::Serialize;

/// How far heavy use can lift an item above its text-match score.
/// At 0.5 a daily-driver app beats a slightly better textual match,
/// but never a much better one.
const MAX_FRECENCY_BOOST: f64 = 0.5;

/// One search result.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Hit {
    pub id: String,
    pub title: String,
    pub kind: String,
    /// Positions (in characters) of the title that matched the query,
    /// ascending, for highlighting.
    pub matched: Vec<u32>,
}

pub struct Engine {
    items: Vec<Item>,
    /// Titles pre-converted for the matcher, parallel to `items`.
    haystacks: Vec<Utf32String>,
    matcher: Matcher,
    /// Reused between searches so a keystroke allocates nothing for scoring.
    scratch: Vec<(u32, u32)>,
}

impl Engine {
    pub fn new(items: Vec<Item>) -> Self {
        let haystacks = items
            .iter()
            .map(|item| Utf32String::from(item.title.as_str()))
            .collect();
        Self {
            scratch: Vec::with_capacity(items.len()),
            items,
            haystacks,
            matcher: Matcher::new(Config::DEFAULT),
        }
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn get(&self, id: &str) -> Option<&Item> {
        self.items.iter().find(|item| item.id == id)
    }

    /// The best `limit` matches for `query`, best first.
    ///
    /// An empty query returns the most-used items, then the rest alphabetically,
    /// so the launcher is useful before anything is typed.
    pub fn search(&mut self, query: &str, limit: usize, usage: &Usage, now: u64) -> Vec<Hit> {
        let query = query.trim();
        if query.is_empty() {
            return self.default_list(limit, usage, now);
        }

        let pattern = Pattern::new(
            query,
            CaseMatching::Ignore,
            Normalization::Smart,
            AtomKind::Fuzzy,
        );

        self.scratch.clear();
        for (index, haystack) in self.haystacks.iter().enumerate() {
            let Some(score) = pattern.score(haystack.slice(..), &mut self.matcher) else {
                continue;
            };
            let boosted = if usage.is_empty() {
                score
            } else {
                let boost = (usage.weight(&self.items[index].id, now) * 0.2).min(MAX_FRECENCY_BOOST);
                (score as f64 * (1.0 + boost)) as u32
            };
            self.scratch.push((boosted, index as u32));
        }

        // Highest score first; ties keep index order, which is alphabetical.
        let by_rank = |a: &(u32, u32), b: &(u32, u32)| b.0.cmp(&a.0).then(a.1.cmp(&b.1));
        if self.scratch.len() > limit {
            self.scratch.select_nth_unstable_by(limit, by_rank);
            self.scratch.truncate(limit);
        }
        self.scratch.sort_unstable_by(by_rank);

        // Match positions are only computed for the rows that will be shown.
        let mut hits = Vec::with_capacity(self.scratch.len());
        let mut matched = Vec::new();
        for &(_, index) in &self.scratch {
            let index = index as usize;
            matched.clear();
            pattern.indices(self.haystacks[index].slice(..), &mut self.matcher, &mut matched);
            matched.sort_unstable();
            matched.dedup();
            hits.push(hit(&self.items[index], matched.clone()));
        }
        hits
    }

    fn default_list(&self, limit: usize, usage: &Usage, now: u64) -> Vec<Hit> {
        let mut order: Vec<(f64, usize)> = self
            .items
            .iter()
            .enumerate()
            .map(|(index, item)| (usage.weight(&item.id, now), index))
            .collect();
        order.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));
        order
            .into_iter()
            .take(limit)
            .map(|(_, index)| hit(&self.items[index], Vec::new()))
            .collect()
    }
}

fn hit(item: &Item, matched: Vec<u32>) -> Hit {
    Hit {
        id: item.id.clone(),
        title: item.title.clone(),
        kind: item.kind.clone(),
        matched,
    }
}

/// Sort items the way the engine expects: by title, ignoring case.
pub fn sort_items(items: &mut [Item]) {
    items.sort_by_cached_key(|item| item.title.to_lowercase());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn engine(titles: &[&str]) -> Engine {
        let mut items: Vec<Item> = titles
            .iter()
            .map(|title| Item::app(*title, format!("/apps/{title}")))
            .collect();
        sort_items(&mut items);
        Engine::new(items)
    }

    fn titles(hits: &[Hit]) -> Vec<&str> {
        hits.iter().map(|hit| hit.title.as_str()).collect()
    }

    #[test]
    fn empty_query_lists_everything_alphabetically() {
        let mut engine = engine(&["Zed", "arc", "Mail"]);
        let hits = engine.search("", 10, &Usage::in_memory(), 0);
        assert_eq!(titles(&hits), ["arc", "Mail", "Zed"]);
    }

    #[test]
    fn empty_query_puts_used_items_first() {
        let mut engine = engine(&["Arc", "Mail", "Zed"]);
        let mut usage = Usage::in_memory();
        usage.record("app:/apps/zed", 100);
        let hits = engine.search("  ", 10, &usage, 100);
        assert_eq!(titles(&hits), ["Zed", "Arc", "Mail"]);
    }

    #[test]
    fn fuzzy_match_ignores_case_and_skips_non_matches() {
        let mut engine = engine(&["Google Chrome", "Calculator", "Notes"]);
        let hits = engine.search("CHR", 10, &Usage::in_memory(), 0);
        assert_eq!(titles(&hits), ["Google Chrome"]);
    }

    #[test]
    fn a_prefix_match_beats_a_scattered_one() {
        let mut engine = engine(&["Arctic Sandal", "Calculator", "Calendar"]);
        let hits = engine.search("cal", 10, &Usage::in_memory(), 0);
        assert_eq!(titles(&hits), ["Calculator", "Calendar", "Arctic Sandal"]);
    }

    #[test]
    fn matched_positions_point_at_the_matching_characters() {
        let mut engine = engine(&["Google Chrome"]);
        let hits = engine.search("chr", 10, &Usage::in_memory(), 0);
        assert_eq!(hits[0].matched, [7, 8, 9]);
    }

    #[test]
    fn heavy_use_breaks_a_tie_between_equal_matches() {
        let mut engine = engine(&["Calculator", "Calendar"]);
        let before = engine.search("cal", 10, &Usage::in_memory(), 0);
        assert_eq!(before[0].title, "Calculator");

        let mut usage = Usage::in_memory();
        for _ in 0..10 {
            usage.record("app:/apps/calendar", 100);
        }
        let after = engine.search("cal", 10, &usage, 100);
        assert_eq!(after[0].title, "Calendar");
    }

    #[test]
    fn results_are_capped_at_the_limit() {
        let names: Vec<String> = (0..200).map(|n| format!("App {n}")).collect();
        let refs: Vec<&str> = names.iter().map(String::as_str).collect();
        let mut engine = engine(&refs);
        assert_eq!(engine.search("app", 25, &Usage::in_memory(), 0).len(), 25);
        assert_eq!(engine.search("", 25, &Usage::in_memory(), 0).len(), 25);
    }

    #[test]
    fn looks_items_up_by_id() {
        let engine = engine(&["Notes"]);
        assert_eq!(engine.get("app:/apps/notes").unwrap().title, "Notes");
        assert!(engine.get("app:/apps/missing").is_none());
    }
}
