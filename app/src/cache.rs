//! On-disk page cache so a host switch or relaunch paints last-known data
//! immediately while a background refresh runs.

use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use chm_core::{
    Health, MergeRow, Overview, QueryRow, ReplicaRow, TableStat, TimeRange, TrafficSeries,
};
use serde::{Deserialize, Serialize};

use crate::pages::Page;

/// Skip a network round-trip when the file is newer than this.
pub const FRESH_SECS: u64 = 20;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CachedPage {
    Overview {
        overview: Overview,
        traffic: TrafficSeries,
    },
    Queries {
        running: Vec<QueryRow>,
        slow: Vec<QueryRow>,
        failed: Vec<QueryRow>,
    },
    Merges(Vec<MergeRow>),
    Replicas(Vec<ReplicaRow>),
    Health(Health),
    Tables(Vec<TableStat>),
    Traffic(TrafficSeries),
}

fn dir() -> Option<PathBuf> {
    dirs::cache_dir().map(|d| d.join("chmonitor").join("pages"))
}

fn file(host: &str, page: Page, range: TimeRange) -> Option<PathBuf> {
    let safe_host: String = host
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    dir().map(|d| {
        d.join(format!(
            "{safe_host}_{}_{}.json",
            page.title().to_ascii_lowercase(),
            range.label()
        ))
    })
}

pub fn load(host: &str, page: Page, range: TimeRange) -> Option<(CachedPage, SystemTime)> {
    let path = file(host, page, range)?;
    let meta = std::fs::metadata(&path).ok()?;
    let mtime = meta.modified().ok()?;
    let text = std::fs::read_to_string(&path).ok()?;
    let page: CachedPage = serde_json::from_str(&text).ok()?;
    Some((page, mtime))
}

pub fn save(host: &str, page: Page, range: TimeRange, data: &CachedPage) {
    let Some(path) = file(host, page, range) else {
        return;
    };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(text) = serde_json::to_string(data) {
        let _ = std::fs::write(path, text);
    }
}

pub fn is_fresh(mtime: SystemTime) -> bool {
    mtime
        .elapsed()
        .map(|d| d < Duration::from_secs(FRESH_SECS))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_health_cache() {
        let host = format!("test-{}", std::process::id());
        let data = CachedPage::Health(Health {
            ok: true,
            readonly_tables: 2,
            ..Health::default()
        });
        save(&host, Page::Health, TimeRange::TwentyFourHours, &data);
        let (loaded, _) = load(&host, Page::Health, TimeRange::TwentyFourHours).expect("cached");
        match loaded {
            CachedPage::Health(h) => {
                assert!(h.ok);
                assert_eq!(h.readonly_tables, 2);
            }
            _ => panic!("wrong page"),
        }
    }
}
