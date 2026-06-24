// Copyright © SixtyFPS GmbH <info@slint.dev>
// SPDX-License-Identifier: GPL-3.0-only OR LicenseRef-Slint-Royalty-free-2.0 OR LicenseRef-Slint-Software-3.0

//! DirectComposition layers for a transparent window.
//!
//! For a transparent window the renderer owns the HWND's composition tree instead of letting wgpu build
//! it from the window handle: a container holding an underlay visual, Slint's surface visual, and an
//! overlay visual, bottom to top. Slint renders into the surface visual (via `CompositionVisual`); the
//! application renders into the underlay (shown through transparent regions) and the overlay (drawn over
//! the chrome). The underlay/overlay visuals and the device are published via
//! [`i_slint_core::graphics::wgpu_29::windows_layers`].
//!
//! `CompositionVisual` surfaces are not auto-committed by wgpu, so the caller commits on the main thread
//! after (re)configuring a swapchain; per-frame `present()` needs no commit.

use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::DirectComposition::{
    DCompositionCreateDevice2, IDCompositionDevice, IDCompositionTarget, IDCompositionVisual,
};
use windows_core::{IUnknown, Interface};

/// The DirectComposition objects for one transparent window, kept alive for the window's lifetime (wgpu
/// and the application take their own references to the visuals they adopt).
pub(crate) struct DCompLayers {
    device: IDCompositionDevice,
    _target: IDCompositionTarget,
    _container: IDCompositionVisual,
    _underlay_visual: IDCompositionVisual,
    _surface_visual: IDCompositionVisual,
    _overlay_visual: IDCompositionVisual,
}

impl DCompLayers {
    /// Commit pending composition-tree changes. Call on the main thread after (re)configuring a swapchain.
    pub(crate) fn commit(&self) {
        if let Err(err) = unsafe { self.device.Commit() } {
            i_slint_core::debug_log!("native-layers: IDCompositionDevice::Commit failed: {err}");
        }
    }
}

/// Build the composition tree for `hwnd`, returning the owner and the surface visual's raw pointer (to
/// wrap as `wgpu::SurfaceTargetUnsafe::CompositionVisual` for Slint's own surface).
pub(crate) fn setup(hwnd: HWND) -> Result<(DCompLayers, *mut core::ffi::c_void), String> {
    unsafe {
        let device: IDCompositionDevice = DCompositionCreateDevice2(None::<&IUnknown>)
            .map_err(|e| format!("DCompositionCreateDevice2: {e}"))?;
        // Non-topmost target — the slot wgpu's DxgiFromVisual would use; we own it instead.
        let target =
            device.CreateTargetForHwnd(hwnd, false).map_err(|e| format!("CreateTargetForHwnd: {e}"))?;
        let container =
            device.CreateVisual().map_err(|e| format!("CreateVisual(container): {e}"))?;
        let underlay_visual =
            device.CreateVisual().map_err(|e| format!("CreateVisual(underlay): {e}"))?;
        let surface_visual =
            device.CreateVisual().map_err(|e| format!("CreateVisual(surface): {e}"))?;
        let overlay_visual =
            device.CreateVisual().map_err(|e| format!("CreateVisual(overlay): {e}"))?;

        // Bottom to top: underlay, Slint's surface, overlay — ordered against an explicit reference visual
        // rather than the null-relative top/bottom form, which proved ambiguous in practice.
        container.AddVisual(&surface_visual, true, None).map_err(|e| format!("AddVisual(surface): {e}"))?;
        container
            .AddVisual(&underlay_visual, false, &surface_visual)
            .map_err(|e| format!("AddVisual(underlay): {e}"))?;
        container
            .AddVisual(&overlay_visual, true, &surface_visual)
            .map_err(|e| format!("AddVisual(overlay): {e}"))?;
        target.SetRoot(&container).map_err(|e| format!("SetRoot: {e}"))?;

        // `as_raw` borrows (no refcount bump); the visuals stay alive via the returned `DCompLayers`.
        let surface_ptr = surface_visual.as_raw();
        i_slint_core::graphics::wgpu_29::windows_layers::publish(
            hwnd.0 as usize,
            i_slint_core::graphics::wgpu_29::windows_layers::LayerHandles {
                underlay: underlay_visual.as_raw() as usize,
                overlay: overlay_visual.as_raw() as usize,
                device: device.as_raw() as usize,
            },
        );

        Ok((
            DCompLayers {
                device,
                _target: target,
                _container: container,
                _underlay_visual: underlay_visual,
                _surface_visual: surface_visual,
                _overlay_visual: overlay_visual,
            },
            surface_ptr,
        ))
    }
}
