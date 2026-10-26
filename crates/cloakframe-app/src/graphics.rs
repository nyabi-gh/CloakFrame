//! Which renderer the window runs on, for the About window and bug reports. wgpu falls back to
//! a software adapter where there is no GPU, and the app to OpenGL where wgpu does not start.

use eframe::egui_wgpu::wgpu;

pub fn of_wgpu(backend: wgpu::Backend, adapter: &str, device: wgpu::DeviceType) -> String {
    let mut text = format!("wgpu, {backend}, {adapter}");
    if device == wgpu::DeviceType::Cpu {
        text.push_str(" (software)");
    }
    text
}

pub fn of_opengl(renderer: &str, version: &str) -> String {
    format!("OpenGL, {renderer}, {version}")
}

pub fn of(cc: &eframe::CreationContext<'_>) -> Option<String> {
    if let Some(state) = &cc.wgpu_render_state {
        let info = state.adapter.get_info();
        return Some(of_wgpu(info.backend, &info.name, info.device_type));
    }
    let gl = cc.gl.as_ref()?;
    use eframe::glow::HasContext;
    #[allow(unsafe_code)]
    // SAFETY: the context is current on this thread while the app is created.
    let (renderer, version) = unsafe {
        (
            gl.get_parameter_string(eframe::glow::RENDERER),
            gl.get_parameter_string(eframe::glow::VERSION),
        )
    };
    Some(of_opengl(&renderer, &version))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_backend_adapter_and_a_software_adapter_are_named() {
        let llvmpipe = |device| {
            of_wgpu(
                wgpu::Backend::Vulkan,
                "llvmpipe (LLVM 19.1.1, 256 bits)",
                device,
            )
        };
        assert_eq!(
            llvmpipe(wgpu::DeviceType::Cpu),
            "wgpu, vulkan, llvmpipe (LLVM 19.1.1, 256 bits) (software)"
        );
        assert_eq!(
            llvmpipe(wgpu::DeviceType::DiscreteGpu),
            "wgpu, vulkan, llvmpipe (LLVM 19.1.1, 256 bits)"
        );
        assert_eq!(
            of_opengl("llvmpipe", "4.5 (Core Profile) Mesa 24.0.5"),
            "OpenGL, llvmpipe, 4.5 (Core Profile) Mesa 24.0.5"
        );
    }
}
