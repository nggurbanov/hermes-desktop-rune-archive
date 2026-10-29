//! Pointer-centered image camera, independent of the encoded source and native view.
#[derive(Clone, Debug)]
pub struct Camera {
    pub viewport: [f32; 2],
    pub image: [u32; 2],
    pub scale: f32,
    pub pan: [f32; 2],
    fitted: bool,
    drag_start: [f32; 2],
}
impl Default for Camera {
    fn default() -> Self {
        Self {
            viewport: [0., 0.],
            image: [0, 0],
            scale: 1.,
            pan: [0., 0.],
            fitted: true,
            drag_start: [0., 0.],
        }
    }
}
impl Camera {
    pub fn layout(&mut self, rect: [f32; 4]) {
        let size = [rect[2].max(0.), rect[3].max(0.)];
        if self.viewport == size {
            return;
        }
        self.viewport = size;
        if self.fitted {
            self.fit();
        } else {
            self.constrain();
        }
    }
    pub fn set_image(&mut self, size: [u32; 2]) -> bool {
        if size == self.image {
            return false;
        }
        self.image = size;
        self.fit();
        true
    }
    pub fn fit(&mut self) {
        let [w, h] = self.image;
        self.scale = if w > 0 && h > 0 {
            ((self.viewport[0] - 32.).max(1.) / w as f32)
                .min((self.viewport[1] - 32.).max(1.) / h as f32)
                .min(1.)
                .max(0.001)
        } else {
            1.
        };
        self.pan = [0., 0.];
        self.fitted = true;
    }
    pub fn actual_size(&mut self) {
        self.scale = 1.;
        self.pan = [0., 0.];
        self.fitted = false;
    }
    pub fn zoom(&mut self, factor: f32, at: Option<[f32; 2]>) {
        if !factor.is_finite() || factor <= 0. || self.image.contains(&0) {
            return;
        }
        let old = self.scale;
        let new = (old * factor).clamp(0.25, 8.);
        let at = at.unwrap_or([self.viewport[0] / 2., self.viewport[1] / 2.]);
        for axis in 0..2 {
            let point = at[axis] - self.viewport[axis] / 2.;
            self.pan[axis] = point - (point - self.pan[axis]) * (new / old);
        }
        self.scale = new;
        self.fitted = false;
        self.constrain();
    }
    pub fn smart_zoom(&mut self, at: [f32; 2]) {
        if self.fitted {
            self.zoom(1. / self.scale, Some(at));
        } else {
            self.fit();
        }
    }
    pub fn wheel(&mut self, dy: f32, at: [f32; 2]) {
        if dy.is_finite() {
            self.zoom((-dy * 0.004).exp().clamp(0.5, 2.), Some(at));
        }
    }
    pub fn drag(&mut self, phase: rune::Phase, dx: f32, dy: f32) {
        if phase == rune::Phase::Down {
            self.drag_start = self.pan;
        }
        if dx.is_finite() && dy.is_finite() {
            self.pan = [self.drag_start[0] + dx, self.drag_start[1] + dy];
            self.fitted = false;
            self.constrain();
        }
    }
    fn constrain(&mut self) {
        for axis in 0..2 {
            let limit =
                ((self.image[axis] as f32 * self.scale + self.viewport[axis]) / 2. - 32.).max(0.);
            self.pan[axis] = self.pan[axis].clamp(-limit, limit);
        }
    }
    pub fn bounds(&self) -> [f32; 4] {
        let w = self.image[0] as f32 * self.scale;
        let h = self.image[1] as f32 * self.scale;
        [
            (self.viewport[0] - w) / 2. + self.pan[0],
            (self.viewport[1] - h) / 2. + self.pan[1],
            w,
            h,
        ]
    }
}

/// Decode untrusted SVG without a filesystem/network resource resolver. Embedded rasters are
/// header-checked before resvg may decode them; a nested SVG cannot load further image resources.
pub(crate) fn rasterize_svg(bytes: &[u8], maximum: u32) -> Result<(u32, u32, Vec<u8>), String> {
    use resvg::{tiny_skia, usvg};
    if bytes.len() > 8 * 1024 * 1024 || bytes.starts_with(&[0x1f, 0x8b]) {
        return Err(
            "The SVG exceeds the safe preview limit or uses unsupported compression".into(),
        );
    }
    static FONTS: std::sync::OnceLock<std::sync::Arc<usvg::fontdb::Database>> =
        std::sync::OnceLock::new();
    let fonts = FONTS.get_or_init(|| {
        let mut database = usvg::fontdb::Database::new();
        for font in rune_theme_opencode::FONTS {
            database.load_font_data(font.to_vec());
        }
        database.set_sans_serif_family("Inter");
        database.set_serif_family("Inter");
        database.set_monospace_family("IBM Plex Mono");
        std::sync::Arc::new(database)
    });
    let mut options = usvg::Options::default();
    options.fontdb = fonts.clone();
    options.font_family = "Inter".into();
    options.image_href_resolver.resolve_string = Box::new(|_, _| None);
    let embedded = usvg::ImageHrefResolver::default_data_resolver();
    // All resources retained in this document share one decoded budget. Per-image bounds alone
    // would still admit thousands of individually small compressed raster allocations.
    let budget = std::sync::Mutex::new((0u64, 0u32));
    options.image_href_resolver.resolve_data = Box::new(move |mime, data, options| {
        let mut budget = budget.lock().ok()?;
        if budget.1 >= 64 {
            return None;
        }
        budget.1 += 1;
        if data.len() > 8 * 1024 * 1024 {
            return None;
        }
        let dimensions = ::image::ImageReader::new(std::io::Cursor::new(data.as_slice()))
            .with_guessed_format()
            .ok()
            .and_then(|r| r.into_dimensions().ok());
        let charge = if let Some((w, h)) = dimensions {
            if w == 0
                || h == 0
                || w > 4096
                || h > 4096
                || w as u64 * h as u64 * 4 > 16 * 1024 * 1024
            {
                return None;
            }
            w as u64 * h as u64 * 4
        } else {
            // usvg strips every image from sub-SVGs. Do not allow gzip expansion here.
            if data.len() > 1024 * 1024
                || data.starts_with(&[0x1f, 0x8b])
                || std::str::from_utf8(&data).is_err()
            {
                return None;
            }
            (data.len() as u64 * 32).max(64 * 1024)
        };
        let retained = budget.0.checked_add(charge)?;
        if retained > 64 * 1024 * 1024 {
            return None;
        }
        budget.0 = retained;
        drop(budget);
        embedded(mime, data, options)
    });
    let tree = usvg::Tree::from_data(bytes, &options).map_err(|_| "The SVG could not be parsed")?;
    let size = tree.size();
    let maximum = maximum.clamp(1, 4096);
    let scale = maximum as f32 / size.width().max(size.height());
    let width = (size.width() * scale).round().clamp(1., maximum as f32) as u32;
    let height = (size.height() * scale).round().clamp(1., maximum as f32) as u32;
    let mut pixmap = tiny_skia::Pixmap::new(width, height)
        .ok_or("The SVG preview bitmap could not be allocated")?;
    resvg::render(
        &tree,
        tiny_skia::Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );
    let rgba = pixmap
        .pixels()
        .iter()
        .flat_map(|pixel| {
            let pixel = pixel.demultiply();
            [pixel.red(), pixel.green(), pixel.blue(), pixel.alpha()]
        })
        .collect();
    Ok((width, height, rgba))
}
