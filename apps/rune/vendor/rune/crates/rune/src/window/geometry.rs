//! Immutable physical geometry for native cascade/restore decisions.
#[derive(Clone, Debug, PartialEq)]
pub struct WindowGeometry {
    /// Outer-frame position when the platform exposes it. Wayland generally does not.
    pub position: Option<(i32, i32)>,
    pub inner_size: (u32, u32),
    pub scale_factor: f64,
    pub maximized: bool,
    /// Current monitor bounds, not the platform's reserved desktop work area.
    pub monitor: Option<MonitorGeometry>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MonitorGeometry {
    pub position: (i32, i32),
    pub size: (u32, u32),
}
impl super::WindowHandle {
    /// Request the native maximized state. The window manager decides whether to honor it.
    pub fn set_maximized(&self, maximized: bool) -> Result<(), String> {
        self.window()?.set_maximized(maximized);
        Ok(())
    }

    /// Snapshot a live native window. Returns None before creation and after destruction.
    /// Monitor bounds exclude no taskbar/Dock area; callers must not claim work-area support.
    pub fn geometry(&self) -> Option<WindowGeometry> {
        let window = self.window().ok()?;
        let size = window.inner_size();
        Some(WindowGeometry {
            position: window.outer_position().ok().map(|p| (p.x, p.y)),
            inner_size: (size.width, size.height),
            scale_factor: window.scale_factor(),
            maximized: window.is_maximized(),
            monitor: window.current_monitor().map(|m| MonitorGeometry {
                position: (m.position().x, m.position().y),
                size: (m.size().width, m.size().height),
            }),
        })
    }
}
