//! Decoded images by URL, shared by every element that shows the same one.

use std::collections::HashMap;

use crate::node::ImageData;

/// How many bytes of decoded raster images no element uses any more the
/// cache keeps, so an image shown again soon (a list scrolled back, a page
/// revisited) needn't be fetched and decoded again.
const UNUSED_BUDGET: usize = 16 * 1024 * 1024;

struct Entry {
    image: ImageData,
    last_used: u64,
}

/// Decoded images by URL. Images an element still shows stay; of the rest,
/// the most recently used are kept up to [`UNUSED_BUDGET`] and the others
/// are dropped, so a long scroll through images doesn't keep every one it
/// ever showed.
#[derive(Default)]
pub(crate) struct ImageCache {
    entries: HashMap<String, Entry>,
    clock: u64,
}

impl ImageCache {
    pub(crate) fn get(&mut self, url: &str) -> Option<&ImageData> {
        self.clock += 1;
        let entry = self.entries.get_mut(url)?;
        entry.last_used = self.clock;
        Some(&entry.image)
    }

    pub(crate) fn insert(&mut self, url: String, image: ImageData) {
        self.clock += 1;
        let last_used = self.clock;
        self.entries.insert(url, Entry { image, last_used });
    }

    /// Drops the least recently used images no element uses beyond the
    /// budget. Call it after handing a newly loaded image to its elements.
    pub(crate) fn trim(&mut self) {
        self.trim_to(UNUSED_BUDGET);
    }

    fn trim_to(&mut self, budget: usize) {
        let mut unused: Vec<(u64, usize, &String)> = self
            .entries
            .iter()
            .filter_map(|(url, entry)| {
                unused_bytes(&entry.image).map(|bytes| (entry.last_used, bytes, url))
            })
            .collect();
        if unused.iter().map(|&(_, bytes, _)| bytes).sum::<usize>() <= budget {
            return;
        }
        unused.sort_unstable_by(|a, b| b.0.cmp(&a.0));
        let mut kept = 0;
        let drop: Vec<String> = unused
            .into_iter()
            .filter(|&(_, bytes, _)| {
                kept += bytes;
                kept > budget
            })
            .map(|(_, _, url)| url.clone())
            .collect();
        for url in drop {
            self.entries.remove(&url);
        }
    }

    #[cfg(test)]
    fn contains(&self, url: &str) -> bool {
        self.entries.contains_key(url)
    }
}

/// A raster image's size when only the cache holds it.
fn unused_bytes(image: &ImageData) -> Option<usize> {
    match image {
        ImageData::Raster(raster) if raster.data.strong_count() == 1 => Some(raster.data.len()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::node::RasterImageData;
    use std::sync::Arc;

    fn image(bytes: usize) -> ImageData {
        ImageData::Raster(RasterImageData::new(1, 1, Arc::new(vec![0; bytes])))
    }

    #[test]
    fn keeps_the_most_recently_used_unused_images_within_the_budget() {
        let mut cache = ImageCache::default();
        cache.insert("a".into(), image(10));
        cache.insert("b".into(), image(10));
        cache.insert("c".into(), image(10));
        cache.get("a");
        cache.trim_to(20);
        assert!(cache.contains("a"));
        assert!(!cache.contains("b"));
        assert!(cache.contains("c"));
    }

    #[test]
    fn never_drops_an_image_an_element_shows() {
        let mut cache = ImageCache::default();
        cache.insert("shown".into(), image(10));
        let shown = cache.get("shown").cloned();
        cache.insert("other".into(), image(10));
        cache.trim_to(0);
        assert!(cache.contains("shown"));
        assert!(!cache.contains("other"));
        drop(shown);
        cache.trim_to(0);
        assert!(!cache.contains("shown"));
    }
}
