use bevy::prelude::*;
use rand::Rng;

use crate::combat::{ApplyDotEvent, DamageEvent};
use crate::loading::TextureAssets;
use crate::{vfx, GameState, GameSet};

pub struct WorldPlugin;

impl Plugin for WorldPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(OnEnter(GameState::Playing), spawn_enemy_and_ui)
            .add_systems(
                Update,
                (handle_damage_events, handle_apply_dot_events, tick_dots)
                    .in_set(GameSet::Sim)
                    .run_if(in_state(GameState::Playing)),
            )
            .add_systems(
                Update,
                (update_enemy_healthbar, animate_damage_numbers)
                    .in_set(GameSet::Ui)
                    .run_if(in_state(GameState::Playing)),
            );
    }
}

#[derive(Component)]
pub struct Enemy;

#[derive(Component)]
pub struct Health {
    pub current: i32,
    pub max: i32,
}

#[derive(Component)]
struct EnemyHpRoot;

#[derive(Component)]
struct EnemyHpFill;

#[derive(Component)]
struct DamageNumber {
    ttl: f32,
    vel: Vec2,
}

#[derive(Component)]
struct DotEffect {
    remaining: f32,
    tick_every: f32,
    tick_accum: f32,
    dps: i32,
}

fn spawn_enemy_and_ui(mut commands: Commands, textures: Res<TextureAssets>) {
    // Enemy sprite
    commands.spawn((
        Sprite::from_image(textures.github.clone()),
        Transform::from_translation(Vec3::new(200.0, 0.0, 0.5)),
        Enemy,
        Health { current: 2000, max: 2000 },
    ));

    // Enemy HP bar at top center
    commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                height: Val::Px(40.0),
                position_type: PositionType::Absolute,
                top: Val::Px(10.0),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            EnemyHpRoot,
        ))
        .with_children(|root| {
            root
                .spawn((
                    Node {
                        width: Val::Px(420.0),
                        height: Val::Px(16.0),
                        justify_content: JustifyContent::FlexStart,
                        align_items: AlignItems::Stretch,
                        ..default()
                    },
                    BackgroundColor(Color::linear_rgb(0.05, 0.05, 0.05)),
                ))
                .with_children(|bar| {
                    bar.spawn((
                        Node { width: Val::Percent(100.0), height: Val::Percent(100.0), ..default() },
                        BackgroundColor(Color::linear_rgb(0.8, 0.2, 0.2)),
                        EnemyHpFill,
                    ));
                });
        });
}

fn handle_damage_events(
    textures: Res<TextureAssets>,
    mut evr: EventReader<DamageEvent>,
    mut q_enemy: Query<(&Transform, &mut Health), With<Enemy>>,
    mut commands: Commands,
) {
    if let Ok((transform, mut hp)) = q_enemy.get_single_mut() {
        for DamageEvent { amount } in evr.read() {
            
            hp.current = (hp.current - *amount).max(0);

            // Spawn floating damage number
            let mut rng = rand::thread_rng();
            let jitter_x: f32 = rng.gen_range(-10.0..10.0);
            let start = transform.translation + Vec3::new(jitter_x, 40.0, 1.0);
            let vel = Vec2::new(0.0, rng.gen_range(30.0..60.0));
            commands.spawn((
                Text2d::new(format!("{}", amount)),
                TextFont { font_size: 22.0, ..default() },
                TextColor(Color::linear_rgb(1.0, 0.9, 0.9)),
                Transform::from_translation(start),
                DamageNumber { ttl: 0.8, vel },
            ));
            vfx::vfx_y2k_stars(&mut commands, &textures, transform.translation);
        //    vfx::vfx_retro_explosion(&mut commands, transform.translation, time.elapsed_secs());
        }
    }
}

fn handle_apply_dot_events(
    mut evr: EventReader<ApplyDotEvent>,
    mut q_enemy: Query<Entity, With<Enemy>>,
    mut commands: Commands,
) {
    if let Ok(enemy) = q_enemy.get_single_mut() {
        for ApplyDotEvent { dps, duration, tick_every } in evr.read() {
            // Add or refresh dot effect
            commands.entity(enemy).insert(DotEffect {
                remaining: *duration,
                tick_every: *tick_every,
                tick_accum: 0.0,
                dps: *dps,
            });
        }
    }
}

fn tick_dots(
    time: Res<Time>,
    mut q: Query<(Entity, &mut DotEffect), With<Enemy>>,
    mut commands: Commands,
    mut writer: EventWriter<DamageEvent>,
) {
    let dt = time.delta_secs();
    for (entity, mut dot) in &mut q {
        dot.remaining -= dt;
        dot.tick_accum += dt;
        while dot.tick_accum >= dot.tick_every {
            dot.tick_accum -= dot.tick_every;
            writer.write(DamageEvent { amount: dot.dps });
        }
        if dot.remaining <= 0.0 {
            commands.entity(entity).remove::<DotEffect>();
        }
    }
}

fn update_enemy_healthbar(
    q_enemy: Query<&Health, With<Enemy>>,
    mut q_fill: Query<&mut Node, With<EnemyHpFill>>,
) {
    if let (Ok(hp), Ok(mut node)) = (q_enemy.get_single(), q_fill.get_single_mut()) {
        let pct = if hp.max > 0 { (hp.current as f32 / hp.max as f32).clamp(0.0, 1.0) } else { 0.0 };
        node.width = Val::Percent(pct * 100.0);
    }
}

fn animate_damage_numbers(
    time: Res<Time>,
    mut q: Query<(Entity, &mut Transform, &mut TextColor, &mut DamageNumber)>,
    mut commands: Commands,
) {
    let dt = time.delta_secs();
    for (e, mut tf, mut color, mut num) in &mut q {
        num.ttl -= dt;
        tf.translation.x += num.vel.x * dt;
        tf.translation.y += num.vel.y * dt;
        let a = (num.ttl / 0.8).clamp(0.0, 1.0);
        color.0 = color.0.with_alpha(a);
        if num.ttl <= 0.0 {
            commands.entity(e).despawn();
        }
    }
}

