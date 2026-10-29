use super::*;
use std::time::{Duration, Instant};
fn vector() -> Arc<[u8]> {
    Arc::from(&br##"<svg xmlns="http://www.w3.org/2000/svg" width="300" height="100"><rect width="300" height="100" fill="#3f9382"/></svg>"##[..])
}
fn wait(images: &mut SafeImages, count: usize) -> Vec<Decoded> {
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut results = vec![];
    while results.len() < count && Instant::now() < deadline {
        results.extend(images.poll());
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(results.len(), count);
    results
}
#[test]
fn worker_decodes_real_images_off_caller_and_gallery_burst_is_bounded_including_completed_results()
{
    let caller = std::thread::current().id();
    let wakes = Arc::new(std::sync::Mutex::new(vec![]));
    let capture = wakes.clone();
    let mut images = SafeImages::new(Arc::new(move || {
        capture.lock().unwrap().push(std::thread::current().id())
    }));
    let mut ids = vec![];
    for _ in 0..32 {
        ids.push(images.request("image/svg+xml", vector(), 360).unwrap());
    }
    assert!(
        images
            .request("image/svg+xml", vector(), 360)
            .unwrap_err()
            .contains("queue")
    );
    let results = wait(&mut images, 32);
    assert!(results.iter().all(|r| ids.contains(&r.id)));
    for result in results {
        assert_eq!(result.result.unwrap().wait().unwrap(), [360, 120]);
    }
    assert!(!wakes.lock().unwrap().is_empty());
    assert!(wakes.lock().unwrap().iter().all(|thread| *thread != caller));
    assert_eq!(images.queued_bytes, 0);
    assert_eq!(images.prepared_bytes, 0);
    assert!(images.outstanding.is_empty());
    assert!(images.request("image/png", vector(), 360).is_ok());
}
#[test]
fn source_and_prepared_budgets_and_cancelled_generations_release_only_when_drained() {
    let mut images = SafeImages::new(Arc::new(|| {}));
    for _ in 0..4 {
        images.request("image/svg+xml", vector(), 2048).unwrap();
    }
    assert!(
        images
            .request("image/svg+xml", vector(), 2048)
            .unwrap_err()
            .contains("queue")
    );
    images.cancel_all();
    let results = wait(&mut images, 4);
    assert!(
        results
            .iter()
            .all(|r| r.result.as_ref().is_err_and(|e| e.contains("cancelled")))
    );
    let bytes: Arc<[u8]> = vec![0; 32 * 1024 * 1024].into();
    images.request("image/png", bytes.clone(), 1).unwrap();
    images.request("image/png", bytes, 1).unwrap();
    assert!(
        images
            .request("image/svg+xml", vector(), 1)
            .unwrap_err()
            .contains("queue")
    );
    images.cancel_all();
    wait(&mut images, 2);
    let id = images.request("image/svg+xml", vector(), 360).unwrap();
    images.cancel(id);
    assert!(
        wait(&mut images, 1)
            .remove(0)
            .result
            .unwrap_err()
            .contains("cancelled")
    );
    assert_eq!(images.queued_bytes, 0);
    assert_eq!(images.prepared_bytes, 0);
}
#[test]
fn content_detection_ignores_mime_and_refuses_bombs_before_decoded_allocation() {
    use ::image::ImageEncoder;
    let rgba = ::image::RgbaImage::from_pixel(1600, 800, ::image::Rgba([25, 140, 90, 255]));
    let mut png = Vec::new();
    ::image::codecs::png::PngEncoder::new(&mut png)
        .write_image(rgba.as_raw(), 1600, 800, ::image::ExtendedColorType::Rgba8)
        .unwrap();
    let prepared = prepare("image/svg+xml", &png, 360).unwrap();
    assert_eq!((prepared.width, prepared.height), (360, 180));
    assert_eq!(prepared.rgba[3], 255);
    let prepared = prepare("image/jpeg", &vector(), 360).unwrap();
    assert_eq!((prepared.width, prepared.height), (360, 120));
    let bomb = include_bytes!("../../fixtures/oversized-preview.png");
    assert!(
        prepare("image/svg+xml", bomb, 360)
            .err()
            .unwrap()
            .contains("safe preview limit")
    );
    assert!(prepare("image/png", b"not an image", 360).is_err());
    let mut directory = crate::media::TempMedia::new();
    let path = directory.reserve("client-private.png").unwrap();
    crate::media::write_private(&path, &png).unwrap();
    let svg = format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="32" height="32"><image href="{}" width="32" height="32"/></svg>"#,
        path.display()
    );
    let prepared = prepare("image/png", svg.as_bytes(), 32).unwrap();
    assert!(prepared.rgba.chunks_exact(4).all(|p| p[3] == 0));
}
#[test]
fn svg_text_uses_only_bundled_inter_and_monospace_fonts() {
    let svg=br##"<svg xmlns="http://www.w3.org/2000/svg" width="300" height="90"><text x="8" y="30" font-family="Inter" font-size="24" fill="#236c62">Hermes diagrams</text><text x="8" y="70" font-family="monospace" font-size="20" fill="#236c62">bot-01: connected</text></svg>"##;
    let image = prepare("image/png", svg, 300).unwrap();
    assert_eq!((image.width, image.height), (300, 90));
    let ink = |rows: std::ops::Range<usize>| {
        rows.flat_map(|y| image.rgba[y * 300 * 4..(y + 1) * 300 * 4].chunks_exact(4))
            .filter(|p| p[3] > 0)
            .count()
    };
    assert!(ink(0..40) > 100, "SVG Inter text disappeared");
    assert!(ink(40..90) > 100, "SVG monospace text disappeared");
}
