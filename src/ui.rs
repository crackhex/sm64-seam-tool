use std::{fs, io::BufWriter, thread};

use crate::{
    edge::{Edge, Orientation, ProjectedPoint, ProjectionAxis},
    float_range::RangeF32,
    game_state::GameState,
    geo::{Point3f, point_f64_to_f32},
    graphics::{
        Camera, GameViewScene, Scene, SeamViewCamera, SeamViewScene, Viewport,
        seam_view_screen_to_world,
    },
    model::{App, ConnectedView, ConnectionMenu, SeamExportForm, SeamViewState},
    seam::PointFilter,
    seam::PointStatusFilter,
    util::get_visible_w_range,
    util::get_visible_w_range_for_seam,
    util::get_visible_y_range,
    util::save_seam_to_csv,
    util::{
        build_game_view_scene, canonicalize_process_name, find_hovered_seam, get_focused_seam_info,
        get_mouse_ray, get_norm_mouse_pos, sync_to_game,
    },
};
use fs::File;
use imgui::{Condition, MouseButton, Ui};
use itertools::Itertools;
use nalgebra::{Point3, Vector3};
use sysinfo::ProcessesToUpdate;

pub fn render_app(ui: &Ui, app: &mut App) -> Vec<Scene> {
    let style_token = ui.push_style_color(imgui::StyleColor::WindowBg, [0.0, 0.0, 0.0, 0.0]);

    let mut scenes = Vec::new();
    ui.window("##app")
        .position([0.0, 0.0], Condition::Always)
        .size(ui.io().display_size, Condition::Always)
        .save_settings(false)
        .resizable(false)
        .title_bar(false)
        .scroll_bar(false)
        .scrollable(false)
        .bring_to_front_on_focus(false)
        .build(|| {
            scenes = match app {
                App::ConnectionMenu(menu) => {
                    if let Some(model) = render_connection_menu(ui, menu) {
                        *app = App::Connected(model);
                    }
                    Vec::new()
                }
                App::Connected(view) => render_connected_view(ui, view),
            }
        });

    style_token.pop();
    scenes
}

fn render_connection_menu(ui: &Ui, menu: &mut ConnectionMenu) -> Option<ConnectedView> {
    menu.system.refresh_processes(ProcessesToUpdate::All, false);
    let processes: Vec<_> = menu
        .system
        .processes()
        .values()
        .sorted_by_key(|process| process.name().to_ascii_lowercase())
        .collect();

    let mut process_index = menu
        .selected_pid
        .and_then(|selected_pid| {
            processes
                .iter()
                .position(|process| process.pid().as_u32() == selected_pid)
        })
        .unwrap_or_else(|| {
            let known_process = processes.iter().position(|process| {
                let name = canonicalize_process_name(process.name().to_str().expect("utf-8"));
                menu.config.base_addresses.contains_key(name.as_str())
            });
            known_process.unwrap_or(0)
        });

    ui.text("Connect to emulator");

    ui.spacing();
    ui.set_next_item_width(300.0);
    ui.combo("##process", &mut process_index, &processes, |process| {
        format!(
            "{:8}: {}",
            process.pid(),
            process.name().to_str().expect("utf-8")
        )
        .into()
    });
    let selected_process = processes.get(process_index).cloned();
    let selected_pid = selected_process.map(|process| process.pid().as_u32());
    let changed_pid = selected_pid != menu.selected_pid;
    menu.selected_pid = selected_pid;

    ui.spacing();
    ui.text("Base address: ");
    ui.same_line_with_pos(110.0);
    ui.set_next_item_width(190.0);
    if ui
        .input_text("##base-addr", &mut menu.base_addr_buffer)
        .build()
    {
        menu.selected_base_addr = parse_int::parse(menu.base_addr_buffer.as_str()).ok();
    }
    if changed_pid {
        if let Some(selected_process) = selected_process {
            if let Some(base_addr) = menu
                .config
                .base_addresses
                .get(canonicalize_process_name(selected_process.name().to_str()?).as_str())
            {
                menu.selected_base_addr = Some(*base_addr);
                menu.base_addr_buffer = format!("{:#X}", *base_addr);
                menu.base_addr_buffer.reserve(32);
            }
        }
    }

    ui.spacing();
    ui.text("Game version: ");
    ui.same_line_with_pos(110.0);
    ui.set_next_item_width(100.0);
    ui.combo(
        "##versions",
        &mut menu.selected_version_index,
        &menu.config.game_versions,
        |game_version| game_version.name.to_string().into(),
    );

    ui.spacing();
    if let Some(pid) = menu.selected_pid {
        if let Some(base_addr) = menu.selected_base_addr {
            if ui.button("Connect") {
                return Some(ConnectedView::new(
                    pid,
                    base_addr,
                    menu.config.game_versions[menu.selected_version_index]
                        .globals
                        .clone(),
                ));
            }
        }
    }

    None
}

fn render_connected_view(ui: &Ui, view: &mut ConnectedView) -> Vec<Scene> {
    if view.sync_to_game {
        sync_to_game(&view.process, &view.globals);
    }

    let state = GameState::read(&view.globals, &view.process);
    view.seam_processor.update(&state);

    let mut scenes = Vec::new();
    ui.child_window("game-view")
        .size([
            0.0,
            if view.seam_view.is_some() {
                ui.window_size()[1] / 2.0
            } else {
                0.0
            },
        ])
        .build(|| {
            scenes.push(Scene::GameView(render_game_view(ui, view, &state)));
        });

    if view.seam_view.is_some() {
        ui.child_window("seam-info").build(|| {
            scenes.push(Scene::SeamView(render_seam_view(ui, view)));
        });
    }

    if view.export_form.is_some() {
        render_export_form(ui, view);
    }

    scenes
}

fn render_game_view(ui: &Ui, view: &mut ConnectedView, state: &GameState) -> GameViewScene {
    let viewport = Viewport {
        x: ui.window_pos()[0],
        y: ui.window_pos()[1],
        width: ui.window_size()[0],
        height: ui.window_size()[1],
    };
    let scene = build_game_view_scene(
        viewport,
        state,
        &view.seam_processor,
        view.hovered_seam.clone(),
    );
    if let Camera::Rotate(camera) = &scene.camera {
        let mouse_ray = get_mouse_ray(ui.io().mouse_pos, ui.window_pos(), ui.window_size(), camera);
        view.hovered_seam = mouse_ray.and_then(|mouse_ray| {
            find_hovered_seam(state, view.seam_processor.active_seams(), mouse_ray)
        });
    }

    if let Some(hovered_seam) = &view.hovered_seam {
        if ui.is_mouse_clicked(MouseButton::Left)
            && !ui.is_any_item_hovered()
            && view.export_form.is_none()
        {
            view.seam_view = Some(SeamViewState::new(hovered_seam.clone()));
        }
    }

    ui.text(view.fps_string.to_string());
    ui.text(format!(
        "remaining: {}",
        view.seam_processor.remaining_seams()
    ));

    ui.checkbox("sync", &mut view.sync_to_game);

    let all_filters = PointFilter::all();
    let mut filter_index = all_filters
        .iter()
        .position(|filter| view.seam_processor.filter() == *filter)
        .unwrap();
    ui.set_next_item_width(100.0);
    if ui.combo("##filter", &mut filter_index, &all_filters, |filter| {
        format!("{}", filter).into()
    }) {
        view.seam_processor.set_filter(all_filters[filter_index]);
    }

    scene
}

fn render_seam_view(ui: &Ui, view: &mut ConnectedView) -> SeamViewScene {
    let seam_view = view.seam_view.as_mut().unwrap();
    let seam = seam_view.seam.clone();

    let viewport = Viewport {
        x: ui.window_pos()[0],
        y: ui.window_pos()[1],
        width: ui.window_size()[0],
        height: ui.window_size()[1],
    };

    let screen_mouse_pos = get_norm_mouse_pos(ui.io().mouse_pos, ui.window_pos(), ui.window_size());
    let screen_mouse_pos = Point3f::new(screen_mouse_pos.0, screen_mouse_pos.1, 0.0);

    let mut camera = get_seam_view_camera(seam_view, &viewport);
    let mut world_mouse_pos = seam_view_screen_to_world(&camera, &viewport, screen_mouse_pos);

    if ui.is_mouse_clicked(MouseButton::Left)
        && !ui.is_any_item_hovered()
        && view.export_form.is_none()
        && screen_mouse_pos.x.abs() <= 1.0
        && screen_mouse_pos.y.abs() <= 1.0
    {
        seam_view.mouse_drag_start_pos = Some(world_mouse_pos);
    }
    if ui.is_mouse_down(MouseButton::Left) {
        if let Some(mouse_drag_start_pos) = seam_view.mouse_drag_start_pos {
            seam_view.camera_pos += mouse_drag_start_pos - world_mouse_pos;
            camera = get_seam_view_camera(seam_view, &viewport);
            world_mouse_pos = seam_view_screen_to_world(&camera, &viewport, screen_mouse_pos);
        }
    } else {
        seam_view.mouse_drag_start_pos = None;
    }

    if !ui.is_any_item_hovered()
        && screen_mouse_pos.x.abs() <= 1.0
        && screen_mouse_pos.y.abs() <= 1.0
    {
        seam_view.zoom += ui.io().mouse_wheel as f64 / 5.0;

        // Move camera to keep world mouse pos the same
        camera = get_seam_view_camera(seam_view, &viewport);
        let new_world_mouse_pos = seam_view_screen_to_world(&camera, &viewport, screen_mouse_pos);
        seam_view.camera_pos += world_mouse_pos - new_world_mouse_pos;

        camera = get_seam_view_camera(seam_view, &viewport);
        world_mouse_pos = seam_view_screen_to_world(&camera, &viewport, screen_mouse_pos);
    }

    let segment_length = camera.span_y as f32 / 100.0;
    let visible_w_range = get_visible_w_range_for_seam(&camera, &viewport, &seam);

    let progress =
        view.seam_processor
            .focused_seam_progress(&seam, visible_w_range, segment_length);

    let mut vertical_grid_lines = Vec::new();
    let mut horizontal_grid_lines = Vec::new();

    let w_range = get_visible_w_range(&camera, &viewport, seam.edge1.projection_axis);
    let (left_w_range, right_w_range) = w_range.cut_out(&RangeF32::inclusive_exclusive(-1.0, 1.0));
    if left_w_range.count() + right_w_range.count() < 100 {
        for w in left_w_range.iter().chain(right_w_range.iter()) {
            vertical_grid_lines.push(Point3::new(w as f64, 0.0, w as f64));
        }
    }

    let y_range = get_visible_y_range(&camera);
    let (left_y_range, right_y_range) = y_range.cut_out(&RangeF32::inclusive_exclusive(-1.0, 1.0));
    if left_y_range.count() + right_y_range.count() < 100 {
        for y in left_y_range.iter().chain(right_y_range.iter()) {
            horizontal_grid_lines.push(Point3::new(0.0, y as f64, 0.0));
        }
    }

    let scene = SeamViewScene {
        viewport,
        camera,
        seam: get_focused_seam_info(&seam, &progress),
        vertical_grid_lines,
        horizontal_grid_lines,
    };

    let close_seam_view = ui.button("Close");

    ui.same_line_with_pos(50.0);
    if let Some(progress) = view.export_progress.lock().unwrap().as_ref() {
        ui.text(format!(
            "Exporting ({:.1}%)",
            progress.complete as f32 / progress.total as f32 * 100.0,
        ));
    } else if ui.button("Export") {
        view.export_form = Some(SeamExportForm::new(
            seam.clone(),
            view.seam_processor.filter(),
        ));
    }

    ui.spacing();

    let rounded_mouse = point_f64_to_f32(world_mouse_pos);
    match seam.edge1.projection_axis {
        ProjectionAxis::X => {
            ui.text(format!("(_, {}, {})", rounded_mouse.y, rounded_mouse.z));
            ui.text(format!(
                "(_, {:#08X}, {:#08X})",
                rounded_mouse.y.to_bits(),
                rounded_mouse.z.to_bits(),
            ));
        }
        ProjectionAxis::Z => {
            ui.text(format!("({}, {}, _)", rounded_mouse.x, rounded_mouse.y));
            ui.text(format!(
                "({:#08X}, {:#08X}, _)",
                rounded_mouse.x.to_bits(),
                rounded_mouse.y.to_bits(),
            ));
        }
    }

    if close_seam_view {
        view.seam_view = None;
    }
    scene
}

fn get_seam_view_camera(seam_view: &mut SeamViewState, viewport: &Viewport) -> SeamViewCamera {
    let seam = &seam_view.seam;

    let w_axis = match seam.edge1.projection_axis {
        ProjectionAxis::X => Vector3::z(),
        ProjectionAxis::Z => Vector3::x(),
    };
    let screen_right = match seam.edge1.orientation {
        Orientation::Positive => -w_axis,
        Orientation::Negative => w_axis,
    };

    let initial_span_y = *seam_view.initial_span_y.get_or_insert_with(|| {
        let w_range = seam.edge1.w_range();
        let y_range = seam.edge1.y_range();
        (y_range.end - y_range.start + 50.0)
            .max((w_range.end - w_range.start + 50.0) * viewport.height / viewport.width)
            as f64
    });
    let span_y = initial_span_y / 2.0f64.powf(seam_view.zoom);

    SeamViewCamera {
        pos: seam_view.camera_pos,
        span_y,
        right_dir: screen_right,
    }
}

fn render_export_form(ui: &Ui, view: &mut ConnectedView) {
    let form = view.export_form.as_mut().unwrap();
    let progress_cell = view.export_progress.clone();

    let style_token = ui.push_style_color(imgui::StyleColor::WindowBg, [0.06, 0.06, 0.06, 0.94]);

    let mut opened = true;
    let mut begun = false;
    ui.window("Export seam data")
        .size([500.0, 300.0], Condition::Appearing)
        .opened(&mut opened)
        .build(|| {
            let show_point =
                |projection_axis: ProjectionAxis, point: ProjectedPoint<i16>| match projection_axis
                {
                    ProjectionAxis::X => format!("(_, {}, {})", point.y, point.w),
                    ProjectionAxis::Z => format!("({}, {}, _)", point.w, point.y),
                };
            let show_edge = |edge: Edge| {
                let normal_info = match (edge.projection_axis, edge.orientation) {
                    (ProjectionAxis::X, Orientation::Positive) => "x+",
                    (ProjectionAxis::X, Orientation::Negative) => "x-",
                    (ProjectionAxis::Z, Orientation::Positive) => "z-",
                    (ProjectionAxis::Z, Orientation::Negative) => "z+",
                };
                format!(
                    "v1 = {}, v2 = {}, n = {}",
                    show_point(edge.projection_axis, edge.vertex1),
                    show_point(edge.projection_axis, edge.vertex2),
                    normal_info,
                )
            };

            ui.text(format!("edge 1: {}", show_edge(form.seam.edge1)));
            ui.text(format!("edge 2: {}", show_edge(form.seam.edge2)));

            ui.separator();

            ui.spacing();
            ui.text("Filename: ");
            ui.same_line_with_pos(80.0);
            ui.set_next_item_width(200.0);
            if ui
                .input_text("##filename", &mut form.filename_buffer)
                .build()
            {
                let filename = form.filename_buffer.as_str().trim();
                if filename.is_empty() {
                    form.filename = None;
                } else {
                    form.filename = Some(filename.to_owned());
                }
            }

            ui.spacing();
            let coord_axis_str = match form.seam.edge1.projection_axis {
                ProjectionAxis::X => "z",
                ProjectionAxis::Z => "x",
            };

            ui.text(format!("min {}: ", coord_axis_str));
            ui.same_line_with_pos(60.0);
            ui.set_next_item_width(100.0);
            if ui.input_text("##min-w", &mut form.min_w_buffer).build() {
                form.min_w = form.min_w_buffer.as_str().parse::<f32>().ok();
            }

            ui.text(format!("max {}: ", coord_axis_str));
            ui.same_line_with_pos(60.0);
            ui.set_next_item_width(100.0);
            if ui.input_text("##max-w", &mut form.max_w_buffer).build() {
                form.max_w = form.max_w_buffer.as_str().parse::<f32>().ok();
            }

            ui.spacing();
            ui.checkbox("Include [-1, 1]", &mut form.include_small_w);

            ui.spacing();
            let all_filters = PointFilter::all();
            let mut filter_index = all_filters
                .iter()
                .position(|filter| form.point_filter == *filter)
                .unwrap();
            ui.set_next_item_width(150.0);
            if ui.combo(
                "##point-filter",
                &mut filter_index,
                &all_filters,
                |filter| format!("{}", filter).into(),
            ) {
                form.point_filter = all_filters[filter_index];
            }

            ui.spacing();
            let all_filters = PointStatusFilter::all();
            let mut filter_index = all_filters
                .iter()
                .position(|filter| form.status_filter == *filter)
                .unwrap();
            ui.set_next_item_width(150.0);
            if ui.combo(
                "##status-filter",
                &mut filter_index,
                &all_filters,
                |filter| format!("{}", filter).into(),
            ) {
                form.status_filter = all_filters[filter_index];
            }

            if let (Some(min_w), Some(max_w), Some(filename)) =
                (form.min_w, form.max_w, form.filename.as_ref())
            {
                (0..3).for_each(|_| ui.spacing());
                if ui.button("Export") {
                    begun = true;

                    let mut writer = BufWriter::new(File::create(filename).unwrap());
                    let seam = form.seam.clone();
                    let point_filter = form.point_filter;
                    let status_filter = form.status_filter;
                    let include_small_w = form.include_small_w;
                    let w_range = RangeF32::inclusive(min_w, max_w);

                    thread::spawn(move || {
                        save_seam_to_csv(
                            &mut writer,
                            |progress| {
                                if let Ok(mut progress_cell) = progress_cell.try_lock() {
                                    *progress_cell = progress
                                }
                            },
                            &seam,
                            point_filter,
                            status_filter,
                            include_small_w,
                            w_range,
                        )
                        .unwrap();
                    });
                }
            }
        });

    if !opened || begun {
        view.export_form = None;
    }

    style_token.pop();
}
