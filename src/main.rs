#![windows_subsystem = "windows"]

use graphics::{ImguiRenderer, Renderer};
use imgui::{ConfigFlags, Context};
use imgui_winit_support::{HiDpiMode, WinitPlatform};
use log::LevelFilter;
use model::App;
use std::time::{Duration, Instant};
use ui::render_app;
use winit::{
    event::{Event, WindowEvent},
    event_loop::{ControlFlow, EventLoop},
    window::WindowBuilder,
};

mod edge;
mod float_range;
mod game_state;
mod geo;
mod graphics;
mod model;
mod process;
mod seam;
mod seam_processor;
mod spatial_partition;
mod ui;
mod util;

fn main() {
    log_panics::init();
    simple_logging::log_to_file("log.txt", LevelFilter::Info).unwrap();

    futures::executor::block_on(async {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::PRIMARY,
            ..Default::default()
        });

        let event_loop = EventLoop::new();
        let window = WindowBuilder::new()
            .with_title("Don't let your seams be seams")
            .build(&event_loop)
            .unwrap();

        let surface = unsafe { instance.create_surface(&window).unwrap() };
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })
            .await
            .expect("no compatible device");
        let (device, queue) = adapter
            .request_device(
                &wgpu::DeviceDescriptor {
                    label: None,
                    features: wgpu::Features::empty(),
                    limits: wgpu::Limits::default(),
                },
                None,
            )
            .await
            .unwrap();

        let output_format = wgpu::TextureFormat::Bgra8Unorm;
        let mut surface_config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: output_format,
            width: window.inner_size().width,
            height: window.inner_size().height,
            present_mode: wgpu::PresentMode::AutoVsync,
            alpha_mode: wgpu::CompositeAlphaMode::Auto,
            view_formats: vec![],
        };
        surface.configure(&device, &surface_config);

        let mut imgui = Context::create();
        imgui.set_ini_filename(None);
        imgui.style_mut().window_rounding = 0.0;
        imgui.style_mut().colors[imgui::StyleColor::WindowBg as usize] = [0.0, 0.0, 0.0, 0.0];
        imgui.io_mut().config_flags |= ConfigFlags::NO_MOUSE_CURSOR_CHANGE;

        let mut platform = WinitPlatform::init(&mut imgui);
        platform.attach_window(imgui.io_mut(), &window, HiDpiMode::Default);

        let imgui_renderer = ImguiRenderer::new(&mut imgui, &device, &queue, surface_config.format);

        let mut renderer = Renderer::new(&device, surface_config.format);
        let mut app = App::new();

        let mut last_fps_time = Instant::now();
        let mut frames_since_fps = 0;

        let mut last_frame = Instant::now();
        event_loop.run(move |event, _, control_flow| {
            let elapsed = last_fps_time.elapsed();
            if elapsed > Duration::from_secs(1) {
                let fps = frames_since_fps as f64 / elapsed.as_secs_f64();
                let mspf = elapsed.as_millis() as f64 / frames_since_fps as f64;

                if let App::Connected(model) = &mut app {
                    model.fps_string = format!("{:.2} mspf = {:.1} fps", mspf, fps);
                }

                last_fps_time = Instant::now();
                frames_since_fps = 0;
            }

            platform.handle_event(imgui.io_mut(), &window, &event);
            match event {
                Event::WindowEvent { event, .. } => match event {
                    WindowEvent::CloseRequested => *control_flow = ControlFlow::Exit,
                    WindowEvent::Resized(size) => {
                        surface_config.width = size.width;
                        surface_config.height = size.height;
                        if surface_config.width > 0 && surface_config.height > 0 {
                            surface.configure(&device, &surface_config);
                        }
                    }
                    _ => {}
                },
                Event::MainEventsCleared => window.request_redraw(),
                Event::RedrawRequested(_) => {
                    if surface_config.width > 0 && surface_config.height > 0 {
                        imgui.io_mut().update_delta_time(last_frame.elapsed());
                        last_frame = Instant::now();

                        platform
                            .prepare_frame(imgui.io_mut(), &window)
                            .expect("Failed to prepare frame");

                        let ui = imgui.frame();
                        let scenes = render_app(&ui, &mut app);
                        platform.prepare_render(&ui, &window);
                        let draw_data = imgui.render();

                        let surface_texture = &surface.get_current_texture().unwrap();
                        let output_view = surface_texture
                            .texture
                            .create_view(&wgpu::TextureViewDescriptor::default());

                        renderer.render(
                            &device,
                            &queue,
                            &output_view,
                            (surface_config.width, surface_config.height),
                            surface_config.format,
                            &scenes,
                        );

                        imgui_renderer.render(
                            &device,
                            &queue,
                            &output_view,
                            (surface_config.width, surface_config.height),
                            draw_data,
                        );

                        frames_since_fps += 1;
                    }
                }
                _ => {}
            }
        });
    })
}
