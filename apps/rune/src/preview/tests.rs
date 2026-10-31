use super::*;
fn target() -> Target {
    Target {
        owner: DraftScope {
            draft_origin: None,
            profile_intent: crate::source::ProfileIntent::Backend,
            connection: 7,
            profile: "research".into(),
            session: Some("s1".into()),
        },
        source: Source::Inline {
            mime: "image/png".into(),
            bytes: Arc::from(&include_bytes!("../../fixtures/oversized-preview.png")[..]),
        },
        label: "large.png".into(),
        kind: Kind::Image,
    }
}
fn settle(service: &mut Service) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(4);
    while service.active().is_some_and(|d| d.loading) && std::time::Instant::now() < deadline {
        service.poll();
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}
#[test]
fn original_is_private_and_survives_bounded_decode_failure_until_scope_retires() {
    let mut service = Service::new();
    service.init(Arc::new(|| {}));
    service.open(
        Connection::Url {
            endpoint: "http://127.0.0.1:1".into(),
            token: None,
        },
        target(),
    );
    settle(&mut service);
    let document = service.active().unwrap();
    assert!(
        document
            .error
            .as_deref()
            .unwrap()
            .contains("safe preview limit")
    );
    let original = document.original.as_ref().unwrap();
    let path = original.path.clone();
    assert!(path.exists());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(
            std::fs::metadata(path.parent().unwrap())
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
    }
    service.close_profile(7, "other");
    assert!(service.active().is_some());
    service.close_profile(7, "research");
    assert!(service.active().is_none());
    assert!(!path.exists());
}
#[test]
fn stale_owners_and_dialog_ids_cannot_replace_or_save_new_preview() {
    let mut service = Service::new();
    service.init(Arc::new(|| {}));
    let connection = Connection::Url {
        endpoint: "http://127.0.0.1:1".into(),
        token: None,
    };
    service.open(connection.clone(), target());
    let old_id = service.active().unwrap().id;
    let owner = target().owner;
    let mut replacement = target();
    replacement.owner.session = Some("s2".into());
    service.open(connection, replacement);
    let new_id = service.active().unwrap().id;
    service
        .send
        .send(Reply::Loaded {
            id: old_id,
            owner,
            result: Err("STALE CONTENT".into()),
        })
        .unwrap();
    service.poll();
    assert_ne!(new_id, old_id);
    assert_ne!(
        service.active().unwrap().error.as_deref(),
        Some("STALE CONTENT")
    );
    service.action(old_id, Action::Close);
    assert!(service.active().is_some());
    service.close_connection(99);
    assert!(service.active().is_some());
    service.close_connection(7);
    assert!(service.active().is_none());
}
#[test]
fn camera_preserves_pointer_anchor_and_native_pan_fit_semantics() {
    let mut camera = image::Camera::default();
    camera.layout([0., 0., 800., 600.]);
    camera.set_image([1600, 800]);
    assert_eq!(camera.scale, 0.48);
    let point = [300., 200.];
    let before = camera.bounds();
    let normalized = [
        (point[0] - before[0]) / before[2],
        (point[1] - before[1]) / before[3],
    ];
    camera.zoom(2., Some(point));
    let after = camera.bounds();
    for axis in 0..2 {
        assert!(((point[axis] - after[axis]) / after[axis + 2] - normalized[axis]).abs() < 0.0001);
    }
    camera.drag(rune::Phase::Down, 0., 0.);
    let pan = camera.pan;
    camera.drag(rune::Phase::Move, 50., -20.);
    assert_eq!(camera.pan, [pan[0] + 50., pan[1] - 20.]);
    camera.fit();
    assert_eq!(camera.pan, [0., 0.]);
    camera.actual_size();
    assert_eq!(camera.scale, 1.);
    camera.zoom(f32::NAN, None);
    assert_eq!(camera.scale, 1.);
    camera.zoom(1e10, None);
    assert_eq!(camera.scale, 8.);
    camera.zoom(1e-10, None);
    assert_eq!(camera.scale, 0.25);
}

#[test]
fn svg_external_image_paths_are_denied_and_embedded_bombs_are_not_decoded() {
    use base64::Engine;
    let mut directory = crate::media::TempMedia::new();
    let path = directory.reserve("private.png").unwrap();
    let mut encoded = std::io::Cursor::new(Vec::new());
    ::image::DynamicImage::ImageRgba8(::image::RgbaImage::from_pixel(
        1,
        1,
        ::image::Rgba([200, 20, 180, 255]),
    ))
    .write_to(&mut encoded, ::image::ImageFormat::Png)
    .unwrap();
    crate::media::write_private(&path, &encoded.into_inner()).unwrap();
    let external = format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="32" height="32"><image href="{}" width="32" height="32"/></svg>"#,
        path.display()
    );
    let (w, h, pixels) = image::rasterize_svg(external.as_bytes(), 32).unwrap();
    assert_eq!((w, h), (32, 32));
    assert!(
        pixels.chunks_exact(4).all(|p| p[3] == 0),
        "An agent SVG loaded a client-local file"
    );
    let bomb = base64::engine::general_purpose::STANDARD
        .encode(include_bytes!("../../fixtures/oversized-preview.png"));
    let embedded = format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="32" height="32"><image href="data:image/png;base64,{bomb}" width="32" height="32"/></svg>"#
    );
    let (_, _, pixels) = image::rasterize_svg(embedded.as_bytes(), 32).unwrap();
    assert!(
        pixels.chunks_exact(4).all(|p| p[3] == 0),
        "An oversized embedded raster was decoded"
    );
    let vector=br##"<svg xmlns="http://www.w3.org/2000/svg" width="300" height="100"><rect width="300" height="100" fill="#1d8f7a"/></svg>"##;
    let (w, h, pixels) = image::rasterize_svg(vector, 2048).unwrap();
    assert_eq!((w, h), (2048, 683));
    assert!(pixels.chunks_exact(4).any(|p| p[3] == 255));
}

#[test]
fn svg_embedded_resources_share_one_decoded_budget_and_count_limit() {
    use ::image::ImageEncoder;
    use base64::Engine;
    let mut svg =
        String::from(r#"<svg xmlns="http://www.w3.org/2000/svg" width="50" height="10">"#);
    for i in 0..5u8 {
        let color = [30 + i * 30, 80 + i * 20, 130 + i * 10, 255];
        let pixels = ::image::RgbaImage::from_pixel(2048, 2048, ::image::Rgba(color));
        let mut encoded = Vec::new();
        ::image::codecs::png::PngEncoder::new_with_quality(
            &mut encoded,
            ::image::codecs::png::CompressionType::Best,
            ::image::codecs::png::FilterType::Sub,
        )
        .write_image(
            pixels.as_raw(),
            2048,
            2048,
            ::image::ExtendedColorType::Rgba8,
        )
        .unwrap();
        let encoded = base64::engine::general_purpose::STANDARD.encode(encoded);
        svg.push_str(&format!(r#"<image x="{}" y="0" width="10" height="10" href="data:image/png;base64,{encoded}"/>"#,i*10));
    }
    svg.push_str("</svg>");
    assert!(svg.len() < 8 * 1024 * 1024);
    let (width, height, pixels) = image::rasterize_svg(svg.as_bytes(), 50).unwrap();
    assert_eq!((width, height), (50, 10));
    for i in 0..4usize {
        let offset = (5 * width as usize + i * 10 + 5) * 4;
        assert_eq!(
            pixels[offset + 3],
            255,
            "An image within the 64 MiB document budget disappeared"
        );
        assert!((pixels[offset] as i32 - (30 + i as i32 * 30)).abs() <= 1);
    }
    let last = (5 * width as usize + 45) * 4;
    assert_eq!(
        pixels[last + 3],
        0,
        "The fifth 16 MiB raster exceeded the shared decoded budget"
    );
    let mut svg = String::from(r#"<svg xmlns="http://www.w3.org/2000/svg" width="65" height="1">"#);
    for i in 0..65 {
        let nested=base64::engine::general_purpose::STANDARD.encode(format!(r##"<svg xmlns="http://www.w3.org/2000/svg" width="1" height="1"><rect id="resource-{i}" width="1" height="1" fill="#22aa88"/></svg>"##));
        svg.push_str(&format!(r#"<image x="{i}" y="0" width="1" height="1" href="data:image/svg+xml;base64,{nested}"/>"#));
    }
    svg.push_str("</svg>");
    let (_, _, pixels) = image::rasterize_svg(svg.as_bytes(), 65).unwrap();
    assert!(pixels[..64 * 4].chunks_exact(4).all(|p| p[3] == 255));
    assert_eq!(
        pixels[64 * 4 + 3],
        0,
        "The document resource count limit was exceeded"
    );
}
