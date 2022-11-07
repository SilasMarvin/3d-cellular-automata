use bevy::{
    core_pipeline::clear_color::ClearColorConfig,
    diagnostic::{FrameTimeDiagnosticsPlugin, LogDiagnosticsPlugin},
    input::keyboard::KeyboardInput,
    input::ButtonState,
    prelude::*,
    render::view::NoFrustumCulling,
    tasks::{ParallelSlice, TaskPool},
    time::FixedTimestep,
};
use bevy_egui::{egui, EguiContext, EguiPlugin};
use bevy_flycam::{FlyCam, NoCameraPlayerPlugin};
use rand::distributions::Distribution;

pub mod instancing;
use instancing::{CustomMaterialPlugin, InstanceData, InstanceMaterialData};

type CellLocations = [bool; CELL_LOCATIONS_SIZE];

type Paused = bool;

const GAME_SIZE: f32 = 100.;
const CELL_LOCATIONS_SIZE: usize = (GAME_SIZE * GAME_SIZE * GAME_SIZE) as usize;
const CELL_SIZE: f32 = 1.;

struct GameRule {
    neighbors_to_surive: [bool; 27],
    neighbors_to_spawn: [bool; 27],
    spawn_noise_count: i32,
    spawn_noise_radius: i32,
    color_from: Color,
    color_to: Color,
}

impl GameRule {
    pub fn default() -> Self {
        let neighbors_to_surive = Self::to_dense_array(&[5, 6, 7, 8]);
        let neighbors_to_spawn = Self::to_dense_array(&[6, 7, 9]);
        GameRule {
            neighbors_to_surive,
            neighbors_to_spawn,
            spawn_noise_count: 50000,
            spawn_noise_radius: 75,
            color_from: Color::YELLOW,
            color_to: Color::BLUE,
        }
    }

    pub fn to_dense_array(vc: &[u8]) -> [bool; 27] {
        let mut ar = [false; 27];
        for i in vc {
            ar[*i as usize] = true;
        }
        ar
    }
}

fn main() {
    let cell_locations: CellLocations = [false; CELL_LOCATIONS_SIZE];
    let game_rule: GameRule = GameRule::default();
    let paused: Paused = true;
    App::new()
        .add_plugins(DefaultPlugins)
        .add_plugin(CustomMaterialPlugin)
        .add_plugin(NoCameraPlayerPlugin)
        .add_plugin(EguiPlugin)
        .add_startup_system(setup)
        .add_system(cell_location_updater.with_run_criteria(FixedTimestep::step(0.125)))
        .add_system(ui.after(cell_location_updater))
        .add_system(feed_cells)
        .add_system(pause)
        .insert_resource(cell_locations)
        .insert_resource(game_rule)
        .insert_resource(paused)
        .add_plugin(FrameTimeDiagnosticsPlugin::default())
        .add_plugin(LogDiagnosticsPlugin::default())
        .run();
}

fn translate_location_to_index(x: f32, y: f32, z: f32) -> usize {
    let x = ((x / CELL_SIZE).floor() * CELL_SIZE) + (GAME_SIZE / 2.);
    let y = ((y / CELL_SIZE).floor() * CELL_SIZE) + (GAME_SIZE / 2.);
    let z = ((z / CELL_SIZE).floor() * CELL_SIZE) + (GAME_SIZE / 2.);
    (x + GAME_SIZE * y + GAME_SIZE * GAME_SIZE * z) as usize
}

fn translate_index_to_location(index: usize) -> (f32, f32, f32) {
    let i = index as f32;
    let x = i % GAME_SIZE - (GAME_SIZE / 2.);
    let y = (i / GAME_SIZE).floor() % GAME_SIZE - (GAME_SIZE / 2.);
    let z = (i / (GAME_SIZE * GAME_SIZE)).floor() - (GAME_SIZE / 2.);
    (x, y, z)
}

fn get_neighbors(index: i32, cell_locations: &ResMut<CellLocations>) -> i32 {
    let loc = translate_index_to_location(index as usize);
    // All potential neighbors a cell can have
    let locations = [
        (-1., -1., -1.),
        (0., -1., -1.),
        (1., -1., -1.),
        (-1., 0., -1.),
        (0., 0., -1.),
        (1., 0., -1.),
        (-1., 1., -1.),
        (0., 1., -1.),
        (1., 1., -1.),
        (-1., -1., 0.),
        (0., -1., 0.),
        (1., -1., 0.),
        (-1., 0., 0.),
        (1., 0., 0.),
        (-1., 1., 0.),
        (0., 1., 0.),
        (1., 1., 0.),
        (-1., -1., 1.),
        (0., -1., 1.),
        (1., -1., 1.),
        (-1., 0., 1.),
        (0., 0., 1.),
        (1., 0., 1.),
        (-1., 1., 1.),
        (0., 1., 1.),
        (1., 1., 1.),
    ];
    locations.iter().fold(0, |acc, x| {
        if loc.0.abs() + x.0 >= (GAME_SIZE / 2.) - 1.
            || loc.1.abs() + x.1 >= (GAME_SIZE / 2.) - 1.
            || loc.2.abs() + x.2 >= (GAME_SIZE / 2.) - 1.
        {
            return acc;
        }
        let index = translate_location_to_index(loc.0 + x.0, loc.1 + x.1, loc.2 + x.2);
        match cell_locations[index] {
            true => acc + 1,
            false => acc,
        }
    })
}

fn cell_location_updater(
    mut cell_locations: ResMut<CellLocations>,
    game_rule: Res<GameRule>,
    paused: Res<Paused>,
) {
    if *paused {
        return;
    }
    let task_pool = TaskPool::new();
    let max_size = (GAME_SIZE * GAME_SIZE * GAME_SIZE) as i32;
    let chunck_size = ((GAME_SIZE * GAME_SIZE * GAME_SIZE) / 32.) as usize;
    let counts = (0..max_size).collect::<Vec<i32>>();
    let cell_changes = counts.par_chunk_map(&task_pool, chunck_size, |chunck| {
        let mut cells_to_add = Vec::new();
        let mut cells_to_remove = Vec::new();
        for i in chunck {
            let nc = get_neighbors(*i, &cell_locations) as usize;
            if game_rule.neighbors_to_spawn[nc] {
                cells_to_add.push(*i as usize);
            }
            if !game_rule.neighbors_to_surive[nc] {
                cells_to_remove.push(*i as usize);
            }
        }
        (cells_to_add, cells_to_remove)
    });

    for (cells_to_add, cells_to_remove) in cell_changes {
        for i in cells_to_add {
            cell_locations[i] = true;
        }
        for i in cells_to_remove {
            cell_locations[i] = false;
        }
    }
}

fn feed_cells(
    cell_locations: Res<CellLocations>,
    game_rule: Res<GameRule>,
    mut q_instances: Query<&mut InstanceMaterialData>,
) {
    let mut instances = q_instances.get_single_mut().unwrap();
    let x: Vec<InstanceData> = cell_locations
        .iter()
        .enumerate()
        .filter_map(|(index, x)| match x {
            false => None,
            true => {
                let loc = translate_index_to_location(index);
                // let distance = (loc.0.abs() + loc.1.abs() + loc.2.abs()) / (GAME_SIZE * 1.5);
                let distance = loc.0.abs().max(loc.1.abs()).max(loc.2.abs()) / (GAME_SIZE / 2.);
                let r =
                    (1. - distance) * game_rule.color_from.r() + distance * game_rule.color_to.r();
                let g =
                    (1. - distance) * game_rule.color_from.g() + distance * game_rule.color_to.g();
                let b =
                    (1. - distance) * game_rule.color_from.b() + distance * game_rule.color_to.b();
                Some(InstanceData {
                    position: Vec3::new(loc.0, loc.1, loc.2),
                    scale: 1.,
                    color: [r, g, b, 1.],
                })
            }
        })
        .collect();
    *instances = InstanceMaterialData(x);
}

fn pause(mut key_evr: EventReader<KeyboardInput>, mut paused: ResMut<bool>) {
    for ev in key_evr.iter() {
        if ButtonState::Pressed == ev.state && ev.scan_code == 28 {
            *paused = !(*paused);
        }
    }
}

// Literally tantan's color picker code
// https://github.com/TanTanDev/3d_celluar_automata
fn color_picker(ui: &mut egui::Ui, color: &mut Color) {
    let mut c = [
        (color.r() * 255.0) as u8,
        (color.g() * 255.0) as u8,
        (color.b() * 255.0) as u8,
    ];
    egui::color_picker::color_edit_button_srgb(ui, &mut c);
    color.set_r(c[0] as f32 / 255.0);
    color.set_g(c[1] as f32 / 255.0);
    color.set_b(c[2] as f32 / 255.0);
}

fn create_random_spawn_points(
    points: i32,
    center: (i32, i32, i32),
    distance: i32,
) -> Vec<(f32, f32, f32)> {
    let x_start =
        (center.0 - (distance / 2) as i32).clamp(-(GAME_SIZE / 2.) as i32, (GAME_SIZE / 2.) as i32);
    let y_start =
        (center.1 - (distance / 2) as i32).clamp(-(GAME_SIZE / 2.) as i32, (GAME_SIZE / 2.) as i32);
    let z_start =
        (center.2 - (distance / 2) as i32).clamp(-(GAME_SIZE / 2.) as i32, (GAME_SIZE / 2.) as i32);
    let mut rng = rand::thread_rng();
    let x_distro = rand::distributions::Uniform::from(x_start..(x_start + distance));
    let y_distro = rand::distributions::Uniform::from(y_start..(y_start + distance));
    let z_distro = rand::distributions::Uniform::from(z_start..(z_start + distance));
    // Does not matter if there are duplicates
    (0..points)
        .map(|_index| {
            (
                x_distro.sample(&mut rng) as f32,
                y_distro.sample(&mut rng) as f32,
                z_distro.sample(&mut rng) as f32,
            )
        })
        .collect()
}

fn ui(
    mut egui_context: ResMut<EguiContext>,
    q_instances: Query<&InstanceMaterialData>,
    mut game_rule: ResMut<GameRule>,
    mut cell_locations: ResMut<CellLocations>,
    mut paused: ResMut<Paused>,
) {
    let instances = q_instances.get_single().unwrap();
    egui::Window::new("Celluar!").show(egui_context.ctx_mut(), |ui| {
        ui.label("Overview:");
        {
            let cell_count = instances.len();
            ui.label(format!("cells: {}", cell_count));
            ui.checkbox(&mut paused, "Paused");

            if ui.button("reset").clicked() {
                *cell_locations = [false; CELL_LOCATIONS_SIZE];
            }

            if ui.button("spawn noise").clicked() {
                for t in create_random_spawn_points(
                    game_rule.spawn_noise_count,
                    (0, 0, 0),
                    game_rule.spawn_noise_radius,
                ) {
                    let index = translate_location_to_index(t.0, t.1, t.2);
                    cell_locations[index] = true;
                }
            }
            let mut spawn_noise_count = game_rule.spawn_noise_count as f32;
            ui.add(
                egui::Slider::new(&mut spawn_noise_count, 1.0..=1000000.0).text("cells to spawn"),
            );
            game_rule.spawn_noise_count = spawn_noise_count as i32;

            let mut spawn_noise_radius = game_rule.spawn_noise_radius as f32;
            ui.add(
                egui::Slider::new(&mut spawn_noise_radius, 1.0..=100.0).text("raduis to spawn in"),
            );
            game_rule.spawn_noise_radius = spawn_noise_radius as i32;
        }

        ui.add_space(24.0);
        ui.label("Rules:");
        {
            color_picker(ui, &mut game_rule.color_from);
            color_picker(ui, &mut game_rule.color_to);

            ui.label("Survival Rule: ");
            ui.horizontal_wrapped(|ui| {
                for (index, mut i) in game_rule.neighbors_to_surive.iter_mut().enumerate() {
                    ui.checkbox(&mut i, format!("{}", index));
                }
            });

            ui.label("Spawn Rule: ");
            ui.horizontal_wrapped(|ui| {
                for (index, mut i) in game_rule.neighbors_to_spawn.iter_mut().enumerate() {
                    ui.checkbox(&mut i, format!("{}", index));
                }
            });
        }
    });
}

fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut cell_locations: ResMut<CellLocations>,
) {
    commands.spawn().insert_bundle((
        meshes.add(Mesh::from(shape::Cube { size: CELL_SIZE })),
        Transform::from_xyz(0.0, 0.0, 0.0),
        GlobalTransform::default(),
        InstanceMaterialData(Vec::new()),
        Visibility::default(),
        ComputedVisibility::default(),
        NoFrustumCulling,
    ));

    commands
        .spawn_bundle(Camera3dBundle {
            transform: Transform::from_xyz(-70., 0., 195.).looking_at(Vec3::ZERO, Vec3::Y),
            camera_3d: Camera3d {
                clear_color: ClearColorConfig::Custom(Color::rgb(0., 0., 0.)),
                ..default()
            },
            ..default()
        })
        .insert(FlyCam);

    for t in create_random_spawn_points(1000, (0, 0, 0), 20) {
        let index = translate_location_to_index(t.0, t.1, t.2);
        cell_locations[index] = true;
    }
}
