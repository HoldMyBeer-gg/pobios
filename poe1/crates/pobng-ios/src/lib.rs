//! The iPad host: egui on a Metal layer, driven by Swift.
//!
//! Swift owns the app here, not Rust. That is not a preference — winit calls
//! `UIApplicationMain` and cannot be embedded in an existing app
//! ([winit#802](https://github.com/rust-windowing/winit/issues/802), open
//! since 2019), and iPadOS 27 refuses to launch apps that do not use UIKit's
//! scene lifecycle, which winit does not implement. So eframe cannot host the
//! app on iPad, and this crate replaces the part of it that owns the window.
//!
//! Everything above that is shared: `pob_core::App` draws into a plain
//! `egui::Ui` and knows nothing about who is hosting it.
//!
//! The Swift side:
//!   1. creates an `MTKView`, hands its `CAMetalLayer` to `pobng_start`
//!   2. calls `pobng_frame` from `MTKViewDelegate.draw(in:)`
//!   3. pushes input through the `pobng_*_event` functions
//!   4. calls `pobng_resize` when the scene geometry changes
//!
//! Frames are driven by UIKit rather than by a loop here, which is what lets
//! the app live inside a scene the system controls.

use std::ffi::{c_char, c_void};

/// Everything the host needs to keep between calls.
pub struct Host {
    /// The tree camera. The build itself lives in `pobng-ffi`, which owns the
    /// single engine PoB allows.
    view: pob_core::tree_view::View,
    art: pob_core::tree_art::TreeArt,
    /// Node whose tooltip is pinned open. Tap to pin, tap again to dismiss —
    /// hover cannot work when a lifted finger hovers nothing.
    pinned: Option<u64>,
    /// Nodes matching the search, set from Swift. Empty means no search is
    /// running, which is not the same as a search that matched nothing.
    matches: std::collections::HashSet<u64>,
    /// Node power, normalised to 0..1, set from Swift when a ranking is
    /// shown. Empty otherwise.
    power: std::collections::HashMap<u64, f32>,
    searching: bool,
    ctx: egui::Context,
    renderer: egui_wgpu::Renderer,
    device: wgpu::Device,
    queue: wgpu::Queue,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    /// Input accumulated since the last frame, handed to egui and cleared.
    raw_input: egui::RawInput,
    /// Points-to-pixels, from the view's `contentsScale`.
    scale: f32,
    /// The GPU's real limit, which egui needs telling.
    max_texture_side: usize,
}

/// Start the app on a `CAMetalLayer`.
///
/// `layer` is a `CAMetalLayer` pointer, `support_dir` a writable directory in
/// the app's sandbox for settings, and `data_dir` PoB's `src/` inside the
/// bundle. Returns an opaque handle, or null if the GPU could not be set up
/// — the caller must check.
///
/// # Safety
/// `layer` must be a valid `CAMetalLayer` that outlives the returned handle,
/// and both paths valid NUL-terminated UTF-8.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pobng_start(
    layer: *mut c_void,
    support_dir: *const c_char,
    data_dir: *const c_char,
    width: u32,
    height: u32,
    scale: f32,
) -> *mut c_void {
    if layer.is_null() || support_dir.is_null() || data_dir.is_null() {
        return std::ptr::null_mut();
    }
    let cstr = |p: *const c_char| {
        unsafe { std::ffi::CStr::from_ptr(p) }
            .to_str()
            .ok()
            .map(std::path::PathBuf::from)
    };
    let (Some(dir), Some(data)) = (cstr(support_dir), cstr(data_dir)) else {
        return std::ptr::null_mut();
    };

    // PoB's `src/` is in the app bundle, not at a path baked in at build
    // time. Set before anything touches the engine.
    pob_core::build_state::set_pob_src(data);

    match Host::new(layer, dir, width, height, scale) {
        Some(host) => Box::into_raw(Box::new(host)) as *mut c_void,
        None => std::ptr::null_mut(),
    }
}

/// Draw one frame. Called from `MTKViewDelegate.draw(in:)`.
///
/// Returns how many milliseconds until the next frame is needed, so the host
/// can pause an idle view rather than burning battery at 120Hz on a static
/// screen. `u32::MAX` means nothing is animating.
///
/// # Safety
/// `handle` must come from `pobng_start` and not have been freed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pobng_frame(handle: *mut c_void) -> u32 {
    let Some(host) = (unsafe { (handle as *mut Host).as_mut() }) else {
        return u32::MAX;
    };
    host.frame()
}

/// Tell the app its surface changed size.
///
/// Apple's guidance is explicit that a Metal app should keep rendering at the
/// existing size while a resize is in progress and only reconfigure when it
/// ends, so the host decides when to call this rather than calling it per
/// frame during a drag.
///
/// # Safety
/// `handle` must come from `pobng_start` and not have been freed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pobng_resize(
    handle: *mut c_void,
    width: u32,
    height: u32,
    scale: f32,
) {
    let Some(host) = (unsafe { (handle as *mut Host).as_mut() }) else {
        return;
    };
    host.resize(width, height, scale);
}

/// A touch began, moved, or ended.
///
/// Fed as pointer events rather than egui's own touch events: the UI was
/// built for a mouse, and its controls expect hover, press and release. A
/// finger is treated as the primary button.
///
/// `phase` is 0 began, 1 moved, 2 ended, 3 cancelled. Coordinates are in
/// points, with the origin at the view's top left.
///
/// # Safety
/// `handle` must come from `pobng_start` and not have been freed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pobng_touch(
    handle: *mut c_void,
    phase: u32,
    x: f32,
    y: f32,
) {
    let Some(host) = (unsafe { (handle as *mut Host).as_mut() }) else {
        return;
    };
    host.touch(phase, x, y);
}

/// A two-finger scroll, in points.
///
/// # Safety
/// `handle` must come from `pobng_start` and not have been freed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pobng_scroll(handle: *mut c_void, dx: f32, dy: f32) {
    let Some(host) = (unsafe { (handle as *mut Host).as_mut() }) else {
        return;
    };
    host.raw_input
        .events
        .push(egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: egui::vec2(dx, dy),
            // The gesture recogniser reports a continuous delta rather than
            // discrete start/end, which is what `Move` means here.
            phase: egui::TouchPhase::Move,
            modifiers: egui::Modifiers::NONE,
        });
}

/// A pinch, as a scale factor relative to the gesture's start.
///
/// # Safety
/// `handle` must come from `pobng_start` and not have been freed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pobng_zoom(handle: *mut c_void, factor: f32) {
    let Some(host) = (unsafe { (handle as *mut Host).as_mut() }) else {
        return;
    };
    host.raw_input.events.push(egui::Event::Zoom(factor));
}

/// Highlight nodes matching a search.
///
/// `ids` is a pointer to `count` node ids. An empty list clears the search,
/// which dims nothing — distinct from a search that matched nothing, which
/// dims everything.
///
/// # Safety
/// `handle` must come from `pobng_start`, and `ids` must point at `count`
/// readable `u64`s.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pobng_set_search(
    handle: *mut c_void,
    ids: *const u64,
    count: usize,
    searching: bool,
) {
    let Some(host) = (unsafe { (handle as *mut Host).as_mut() }) else {
        return;
    };
    host.matches.clear();
    if !ids.is_null() {
        let slice = unsafe { std::slice::from_raw_parts(ids, count) };
        host.matches.extend(slice.iter().copied());
    }
    host.searching = searching;
}

/// The node whose tooltip is pinned, or 0. Swift polls this after each
/// frame so a tapped jewel socket can grow a native picker beside the
/// egui tooltip.
///
/// # Safety
/// `handle` must be null or come from `pobng_start` and not yet be passed
/// to `pobng_stop`. Called only from the main thread, like every other
/// entry point here.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pobng_pinned_node(handle: *mut c_void) -> u64 {
    let Some(host) = (unsafe { (handle as *mut Host).as_ref() }) else {
        return 0;
    };
    host.pinned.unwrap_or(0)
}

/// Drop the pinned tooltip, as tapping empty tree does.
///
/// # Safety
/// `handle` must be null or come from `pobng_start` and not yet be passed
/// to `pobng_stop`. Called only from the main thread, like every other
/// entry point here.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pobng_unpin(handle: *mut c_void) {
    if let Some(host) = unsafe { (handle as *mut Host).as_mut() } {
        host.pinned = None;
    }
}

/// Centre the view on a node and pin its tooltip. Zooms in if the view is
/// too far out to read the node.
///
/// # Safety
/// `handle` must be null or come from `pobng_start` and not yet be passed
/// to `pobng_stop`. Called only from the main thread, like every other
/// entry point here.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pobng_focus_node(handle: *mut c_void, id: u64) {
    let Some(host) = (unsafe { (handle as *mut Host).as_mut() }) else {
        return;
    };
    let Some((x, y)) = pobng_ffi::node_position(id) else {
        return;
    };
    host.view.center = (x, y);
    // Close enough to read the node, whatever the view was doing.
    if host.view.zoom < 0.2 {
        host.view.zoom = 0.2;
    }
    host.pinned = Some(id);
    // Light it the way a search hit is lit and dim the rest, so the eye
    // lands on it. Swift restores the real search a few seconds later.
    host.matches.clear();
    host.matches.insert(id);
    host.searching = true;
}

/// Colour nodes by a ranking. `values` are 0..1; an empty list clears it.
///
/// # Safety
/// `handle` must be null or come from `pobng_start` and not yet be passed
/// to `pobng_stop`. `ids` and `values` must each point to `count` readable
/// elements, or be null. Called only from the main thread.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pobng_set_power(
    handle: *mut c_void,
    ids: *const u64,
    values: *const f32,
    count: usize,
) {
    let Some(host) = (unsafe { (handle as *mut Host).as_mut() }) else {
        return;
    };
    host.power.clear();
    if !ids.is_null() && !values.is_null() {
        let ids = unsafe { std::slice::from_raw_parts(ids, count) };
        let values = unsafe { std::slice::from_raw_parts(values, count) };
        host.power.extend(ids.iter().copied().zip(values.iter().copied()));
    }
}

/// Free the app.
///
/// # Safety
/// `handle` must come from `pobng_start`, and must not be used afterwards.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pobng_stop(handle: *mut c_void) {
    if handle.is_null() {
        return;
    }
    drop(unsafe { Box::from_raw(handle as *mut Host) });
}

impl Host {
    fn new(
        layer: *mut c_void,
        support_dir: std::path::PathBuf,
        width: u32,
        height: u32,
        scale: f32,
    ) -> Option<Self> {
        // No display handle on iOS: the layer comes from UIKit, not from a
        // windowing system wgpu would need to talk to.
        let mut desc = wgpu::InstanceDescriptor::new_without_display_handle();
        desc.backends = wgpu::Backends::METAL;
        let instance = wgpu::Instance::new(desc);

        // The layer comes from Swift, and its lifetime is the view's. Treated
        // as 'static because the Swift side must outlive the handle, which is
        // the contract `pobng_start` states.
        let surface = unsafe {
            instance.create_surface_unsafe(
                wgpu::SurfaceTargetUnsafe::CoreAnimationLayer(layer),
            )
        }
        .ok()?;

        let adapter =
            pollster_block(instance.request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::default(),
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
                ..Default::default()
            }))
            .ok()?;

        // The adapter's own limits rather than wgpu's conservative defaults.
        // PoB's tree art includes sheets larger than 2048 on a side —
        // `bloodline-3` is 2329x2329 — and the default limit would reject
        // them.
        let (device, queue) = pollster_block(adapter.request_device(
            &wgpu::DeviceDescriptor {
                label: Some("pobng"),
                required_limits: adapter.limits(),
                ..Default::default()
            },
        ))
        .ok()?;

        let caps = surface.get_capabilities(&adapter);
        let format = caps.formats.first().copied()?;
        // Built from the surface's own defaults so new fields do not have to
        // be tracked by hand every wgpu release.
        let mut config = surface
            .get_default_config(&adapter, width.max(1), height.max(1))?;
        config.format = format;
        config.usage = wgpu::TextureUsages::RENDER_ATTACHMENT;
        config.present_mode = wgpu::PresentMode::Fifo;
        surface.configure(&device, &config);

        let renderer = egui_wgpu::Renderer::new(
            &device,
            format,
            egui_wgpu::RendererOptions::default(),
        );

        let ctx = egui::Context::default();
        let a11y = pob_core::style::A11y::default();
        pob_core::style::apply(&ctx, &a11y);

        // Only the tree camera and its art, not a whole `App`. Building one
        // here would boot a second engine, which PoB does not allow — its own
        // guard caught it on device: "an engine is already running in this
        // process". The engine lives in `pobng-ffi`, which SwiftUI also uses.
        let _ = support_dir;
        let view = pob_core::tree_view::View::default();
        let art = pob_core::tree_art::TreeArt::new(
            &pob_core::build_state::pob_src_path(),
        );

        let max_texture_side = device.limits().max_texture_dimension_2d as usize;

        Some(Self {
            max_texture_side,
            view,
            art,
            pinned: None,
            matches: std::collections::HashSet::new(),
            power: std::collections::HashMap::new(),
            searching: false,
            ctx,
            renderer,
            device,
            queue,
            surface,
            config,
            raw_input: egui::RawInput::default(),
            scale,
        })
    }

    fn touch(&mut self, phase: u32, x: f32, y: f32) {
        let pos = egui::pos2(x, y);
        let events = &mut self.raw_input.events;

        // egui needs to know where the pointer is before it is told about a
        // press, or the click lands wherever the pointer was last frame —
        // which for a touch screen is usually somewhere else entirely.
        events.push(egui::Event::PointerMoved(pos));

        match phase {
            0 => events.push(egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            }),
            2 => {
                events.push(egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: egui::Modifiers::NONE,
                });
                // A finger that has lifted is nowhere, and leaving it where
                // it was leaves every control under it looking hovered.
                events.push(egui::Event::PointerGone);
            }
            3 => events.push(egui::Event::PointerGone),
            _ => {}
        }
    }

    fn resize(&mut self, width: u32, height: u32, scale: f32) {
        if width == 0 || height == 0 {
            return;
        }
        self.config.width = width;
        self.config.height = height;
        self.scale = scale;
        self.surface.configure(&self.device, &self.config);
    }

    fn frame(&mut self) -> u32 {
        let size = egui::vec2(
            self.config.width as f32 / self.scale,
            self.config.height as f32 / self.scale,
        );

        let mut input = std::mem::take(&mut self.raw_input);
        input.screen_rect = Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size));
        // Without this egui assumes 2048 and refuses anything larger, which
        // took the app down on PoB's own tree sheets. eframe reads it from
        // the device the same way; there is nothing iOS-specific about it,
        // it simply has to be told.
        input.max_texture_side = Some(self.max_texture_side);
        // egui works in points; the scale is what turns them into the
        // layer's pixels.
        input.viewports
            .entry(egui::ViewportId::ROOT)
            .or_default()
            .native_pixels_per_point = Some(self.scale);

        // Only the tree. SwiftUI draws everything else — this view is one
        // element inside its layout rather than the whole screen, which is
        // the point of the split.
        let (view, art, pinned) = (&mut self.view, &mut self.art, &mut self.pinned);
        let (matches, searching, power) = (&self.matches, self.searching, &self.power);
        let mut output = self.ctx.run_ui(input, |ui| {
            pobng_ffi::draw_tree(ui, view, art, pinned, matches, searching, power)
        });

        // Textures are applied before the surface is acquired, because
        // `TexturesDelta` panics if it is dropped with anything unapplied —
        // and acquiring can fail. Doing it after meant the first lost surface
        // took the app down with "Dropped TexturesDelta with 1 unapplied
        // deltas", which is what the first launch on the iPad did.
        //
        // One id can carry several deltas in a frame: a font atlas growing
        // arrives as a set of partial updates.
        for (id, deltas) in &output.textures_delta.set {
            for delta in deltas {
                self.renderer
                    .update_texture(&self.device, &self.queue, *id, delta);
            }
        }
        for id in &output.textures_delta.free {
            self.renderer.free_texture(id);
        }
        output.textures_delta.clear();

        let frame = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(frame) => frame,
            // Suboptimal still draws; the size usually settles by itself once
            // a resize finishes.
            wgpu::CurrentSurfaceTexture::Suboptimal(frame) => frame,
            // Lost or timed out mid-resize is ordinary on iPad, where the
            // system resizes the scene under us. Reconfigure and let the next
            // frame have it rather than treating it as an error.
            _ => {
                self.surface.configure(&self.device, &self.config);
                return 16;
            }
        };

        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder =
            self.device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("pobng frame"),
                });

        let tris = self
            .ctx
            .tessellate(output.shapes, output.pixels_per_point);
        let desc = egui_wgpu::ScreenDescriptor {
            size_in_pixels: [self.config.width, self.config.height],
            pixels_per_point: output.pixels_per_point,
        };
        self.renderer.update_buffers(
            &self.device,
            &self.queue,
            &mut encoder,
            &tris,
            &desc,
        );

        {
            let pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("pobng egui"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.02,
                            g: 0.03,
                            b: 0.047,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            self.renderer
                .render(&mut pass.forget_lifetime(), &tris, &desc);
        }

        self.queue.submit(Some(encoder.finish()));
        self.queue.present(frame);

        output
            .viewport_output
            .get(&egui::ViewportId::ROOT)
            .map(|v| v.repaint_delay.as_millis().min(u32::MAX as u128) as u32)
            .unwrap_or(u32::MAX)
    }

}

/// Block on a future without pulling in an async runtime.
///
/// wgpu's setup calls are async but resolve immediately on Metal, so a full
/// executor would be weight for nothing.
fn pollster_block<F: std::future::Future>(mut fut: F) -> F::Output {
    use std::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};

    fn noop(_: *const ()) {}
    fn clone(_: *const ()) -> RawWaker {
        RawWaker::new(std::ptr::null(), &VTABLE)
    }
    static VTABLE: RawWakerVTable = RawWakerVTable::new(clone, noop, noop, noop);

    let waker = unsafe { Waker::from_raw(RawWaker::new(std::ptr::null(), &VTABLE)) };
    let mut cx = Context::from_waker(&waker);
    let mut fut = unsafe { std::pin::Pin::new_unchecked(&mut fut) };
    loop {
        if let Poll::Ready(out) = fut.as_mut().poll(&mut cx) {
            return out;
        }
        std::hint::spin_loop();
    }
}
