//! Durable row anchors survive latest-page offset drift and destructive rewrites.
use crate::gateway::HistoryMessage;
use std::collections::{BTreeMap, HashMap};

pub(super) fn older(
    existing: Vec<HistoryMessage>,
    page: Vec<HistoryMessage>,
) -> Vec<HistoryMessage> {
    if existing.is_empty() || page.is_empty() {
        return existing;
    }
    let held: HashMap<_, _> = existing
        .iter()
        .enumerate()
        .filter_map(|(i, row)| row.row_id().map(|id| (id, i)))
        .collect();
    let mut slots: BTreeMap<usize, Vec<HistoryMessage>> = BTreeMap::new();
    let mut pending = vec![];
    let mut last = None;
    for row in page {
        if let Some(&index) = row.row_id().and_then(|id| held.get(&id)) {
            slots.entry(index).or_default().append(&mut pending);
            last = Some(index);
        } else {
            pending.push(row);
        }
    }
    slots
        .entry(last.map_or(0, |i| i + 1))
        .or_default()
        .append(&mut pending);
    let mut merged = vec![];
    for (i, row) in existing.into_iter().enumerate() {
        merged.extend(slots.remove(&i).unwrap_or_default());
        merged.push(row);
    }
    for rows in slots.into_values() {
        merged.extend(rows);
    }
    merged
}

pub(super) fn tail(
    previous: Vec<HistoryMessage>,
    page: Vec<HistoryMessage>,
    partial: bool,
    same_authority: bool,
) -> Vec<HistoryMessage> {
    if !partial || !same_authority || page.is_empty() {
        return page;
    }
    let fresh: HashMap<_, _> = page
        .iter()
        .filter_map(|r| r.row_id().map(|id| (id, ())))
        .collect();
    let Some(anchor) = previous
        .iter()
        .position(|r| r.row_id().is_some_and(|id| fresh.contains_key(&id)))
    else {
        return page;
    };
    let first = page.iter().filter_map(HistoryMessage::row_id).min();
    let mut prefix: Vec<_> = previous
        .into_iter()
        .take(anchor)
        .filter(|r| {
            r.row_id()
                .is_some_and(|id| first.is_some_and(|first| id < first))
        })
        .collect();
    prefix.extend(page);
    prefix
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn rows(ids: &[u64]) -> Vec<HistoryMessage> {
        ids.iter()
            .map(|id| {
                HistoryMessage::from_row(
                    &json!({"role":"user","row_id":id,"text":format!("row {id}")}),
                )
            })
            .collect()
    }
    fn ids(rows: &[HistoryMessage]) -> Vec<u64> {
        rows.iter().filter_map(HistoryMessage::row_id).collect()
    }
    #[test]
    fn backfill_places_offset_drift_on_both_sides_of_shared_rows() {
        assert_eq!(
            ids(&older(rows(&[5, 7]), rows(&[2, 5, 6, 7, 9]))),
            [2, 5, 6, 7, 9]
        );
        assert_eq!(ids(&older(rows(&[5, 7]), rows(&[1, 2]))), [1, 2, 5, 7]);
        assert!(older(vec![], rows(&[1, 2])).is_empty());
    }
    #[test]
    fn refresh_preserves_backfill_but_removes_rewound_tail_and_changed_authority() {
        assert_eq!(
            ids(&tail(rows(&[1, 2, 3, 4, 5]), rows(&[3, 4, 6]), true, true)),
            [1, 2, 3, 4, 6]
        );
        assert_eq!(
            ids(&tail(rows(&[1, 2, 3, 4, 5]), rows(&[3, 4]), true, false)),
            [3, 4]
        );
        assert_eq!(
            ids(&tail(rows(&[1, 2, 3]), rows(&[8, 9]), true, true)),
            [8, 9]
        );
        assert_eq!(
            ids(&tail(rows(&[1, 2, 3]), rows(&[2, 3]), false, true)),
            [2, 3]
        );
    }
}
