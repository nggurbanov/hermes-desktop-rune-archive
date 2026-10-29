//! Shared bot appearance follows the external profile's actual metadata and asset store.
use crate::pages::{PageEffect, PageRequest, segment};
use rune::{Image, ImageCache, Waker};
use serde_json::{Value, json};
use std::{collections::BTreeMap, sync::Arc};
pub struct Identity {
    pub shape: String,
    pub color: String,
    pub image: Option<Image>,
}
#[derive(Default)]
pub struct Identities {
    pub rows: BTreeMap<String, Identity>,
    cache: Option<ImageCache>,
    epoch: Option<u64>,
}
impl Identities {
    pub fn init(&mut self, waker: Waker) {
        self.cache = Some(ImageCache::new(waker));
    }
    pub fn bind(&mut self, epoch: u64) -> Vec<PageEffect> {
        if self.epoch != Some(epoch) {
            self.rows.clear();
            self.epoch = Some(epoch);
        }
        vec![(
            "identities.roster".into(),
            PageRequest::rpc("profiles.list", json!({})),
        )]
    }
    pub fn seed(&mut self, rows: &[Value]) -> Vec<PageEffect> {
        let mut next = BTreeMap::new();
        let mut effects = vec![];
        for row in rows {
            let Some(name) = row["name"].as_str().filter(|n| !n.is_empty()) else {
                continue;
            };
            let meta = &row["ui_meta"]["hermes-bots"];
            let image = self.rows.remove(name).and_then(|r| r.image);
            next.insert(
                name.into(),
                Identity {
                    shape: meta["shape"].as_str().unwrap_or("circle").into(),
                    color: meta["color"].as_str().unwrap_or("").into(),
                    image,
                },
            );
            effects.push((
                format!("identities.avatar.{}", segment(name)),
                PageRequest::rpc("profiles.get_asset", json!({"name":name,"asset":"avatar"}))
                    .profile(name),
            ));
        }
        self.rows = next;
        effects
    }
    pub fn apply(&mut self, op: &str, result: Result<Value, String>) -> Vec<PageEffect> {
        let Ok(v) = result else {
            return vec![];
        };
        if op == "identities.roster" {
            if let Some(rows) = v["profiles"].as_array() {
                return self.seed(rows);
            }
            return vec![];
        }
        if let Some(name) = op.strip_prefix("identities.avatar.") {
            let name =
                url::form_urlencoded::parse(format!("n={}", name.replace('+', "%2B")).as_bytes())
                    .next()
                    .map(|(_, v)| v.into_owned())
                    .unwrap_or_default();
            if let Some(row) = self.rows.get_mut(&name) {
                if v["found"] == false {
                    row.image = None;
                } else if v["found"] == true {
                    if let Some(data) = v["data"].as_str() {
                        if let Ok(bytes) = crate::pages::bots::decode_avatar(data) {
                            row.image = self
                                .cache
                                .as_ref()
                                .map(|c| c.load(Arc::<[u8]>::from(bytes)));
                        }
                    }
                }
            }
        }
        vec![]
    }
    pub fn image(&self, name: &str, size: f32) -> Option<Image> {
        let row = self.rows.get(name)?;
        if let Some(image) = &row.image {
            return Some(image.clone());
        }
        self.cache.as_ref().map(|c| {
            c.svg(
                Arc::<[u8]>::from(
                    crate::pages::bots::faces::face_svg(&row.shape, &row.color, name).into_bytes(),
                ),
                size,
                2.,
            )
        })
    }
}
