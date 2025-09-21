use bevy::prelude::*;
use crate::GameSet;
use crate::loading::TextureAssets;

// 2D VFX port for Bevy 0.16

pub struct VfxPlugin;

impl Plugin for VfxPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (
                retro_explosion_system,
                tick_vfx_particles,
                tick_vfx_flash,
                tick_y2k_stars,
            )
                .in_set(GameSet::Ui),
        );
    }
}

// Public API — spawn an explosion at a world position
pub fn vfx_retro_explosion(commands: &mut Commands, origin: Vec3, time: f32) {
    commands.spawn(VfxExplosion {
        origin,
        time_spawned: time,
        last_emitted: -1.0,
        color: Color::linear_rgb(1.0, 0.6, 0.8),
    });

    vfx_retro_explosion_flash(commands, origin, Color::linear_rgb(1.0, 0.6, 0.8));
}

// Components and systems

#[derive(Component)]
struct VfxExplosion {
    origin: Vec3,
    time_spawned: f32,
    last_emitted: f32,
    color: Color,
}

#[derive(Component)]
struct VfxParticle {
    vel: Vec2,
    ttl: f32,
}

#[derive(Component)]
struct VfxFlash {
    ttl: f32,
}

fn retro_explosion_system(
    mut commands: Commands,
    time: Res<Time>,
    mut q: Query<(Entity, &mut VfxExplosion)>,
) {
    let t = time.elapsed_secs();

    for (e, mut explosion) in &mut q {
        if t - explosion.last_emitted < 0.02 {
            continue;
        }

        if explosion.last_emitted < 0.0 {
            // Initial burst of chunky square particles
            for i in 0..10 {
                let f = i as f32;
                let dir = Vec2::new((f * 2.3).sin(), (f * 5.1).cos()).normalize_or_zero();
                let vel = dir * 120.0;
                commands.spawn((
                    Sprite::from_color(explosion.color, Vec2::splat(6.0)),
                    Transform::from_translation(explosion.origin + Vec3::new(0.0, 0.0, 0.8)),
                    VfxParticle { vel, ttl: 0.35 },
                ));
            }
        }

        explosion.last_emitted = t;

        // Small flickers near the center
        let flicker = Sprite::from_color(explosion.color, Vec2::splat(10.0));
        commands.spawn((
            flicker,
            Transform::from_translation(explosion.origin + Vec3::new(0.0, 0.0, 0.9)),
            VfxParticle { vel: Vec2::ZERO, ttl: 0.06 },
        ));

        // End after a short duration
        if t - explosion.time_spawned > 0.7 {
            commands.entity(e).despawn();
        }
    }
}

fn tick_vfx_particles(
    time: Res<Time>,
    mut q: Query<(Entity, &mut Transform, &mut Sprite, &mut VfxParticle)>,
    mut commands: Commands,
) {
    let dt = time.delta_secs();
    for (e, mut tf, mut sprite, mut p) in &mut q {
        p.ttl -= dt;
        tf.translation.x += p.vel.x * dt;
        tf.translation.y += p.vel.y * dt;
        // Simple drag
        p.vel *= 0.9_f32.powf(60.0 * dt);
        // Fade and shrink
        let a = (p.ttl / 0.35).clamp(0.0, 1.0);
        sprite.color = sprite.color.with_alpha(a);
        let s = 0.5 + 0.5 * a;
        tf.scale = Vec3::splat(s);
        if p.ttl <= 0.0 {
            commands.entity(e).despawn();
        }
    }
}

pub fn vfx_retro_explosion_flash(commands: &mut Commands, origin: Vec3, color: Color) {
    commands.spawn((
        Sprite::from_color(color, Vec2::splat(90.0)),
        Transform::from_translation(origin + Vec3::new(0.0, 0.0, 0.7)),
        VfxFlash { ttl: 0.12 },
    ));
}

fn tick_vfx_flash(
    time: Res<Time>,
    mut q: Query<(Entity, &mut Transform, &mut Sprite, &mut VfxFlash)>,
    mut commands: Commands,
) {
    let dt = time.delta_secs();
    for (e, mut tf, mut sprite, mut flash) in &mut q {
        flash.ttl -= dt;
        let a = (flash.ttl / 0.12).clamp(0.0, 1.0);
        sprite.color = sprite.color.with_alpha(a);
        tf.scale = Vec3::splat(1.0 + (1.0 - a) * 0.5);
        if flash.ttl <= 0.0 { commands.entity(e).despawn(); }
    }
}

// =========================
// Y2K Stars (2D port)
// =========================

#[derive(Component)]
struct Y2KStar {
    vel: Vec2,
    angle: f32,
    ang_vel: f32,
    size: f32,
    gravity: f32,
    damping: f32,
    blit_index: i32,
}

pub fn vfx_y2k_stars(commands: &mut Commands, textures: &Res<TextureAssets>, origin: Vec3) {
    for i in 0..6 {
        let f = i as f32;
        let rand = Vec2::new((f * 200.0).sin(), (f * 700.0).sin() * 0.5 + 0.5);

        let use_hollow = i % 3 == 0;
        let image = if use_hollow {
            textures.hollowstar.clone()
        } else {
            textures.smallstar.clone()
        };

        let sprite = Sprite::from_image(image);

        let pos = origin + Vec3::new(0.0, 0.0, 0.8);
        let size = (f + 10.0) / 5.0 + 0.5;
        let vel = rand * 25.0 + Vec2::Y * 9.0;

        commands.spawn((
            sprite,
            Transform::from_translation(pos).with_scale(Vec3::splat(size)),
            Y2KStar {
                vel,
                angle: 0.0,
                ang_vel: 6.0,
                size,
                gravity: 35.0,
                damping: 1.0,
                blit_index: i,
            },
        ));
    }
}

fn tick_y2k_stars(
    time: Res<Time>,
    mut q: Query<(Entity, &mut Transform, &mut Sprite, &mut Y2KStar)>,
    mut commands: Commands,
) {
    let t = time.elapsed_secs();
    let dt = time.delta_secs();

    // simple six-color palette similar to DDclone "crazy colors"
    let palette = [
        Color::linear_rgb(0.7, 0.4, 1.0), // violet-ish
        Color::linear_rgb(1.0, 1.0, 1.0),
        Color::linear_rgb(1.0, 0.6, 0.8), // pink-ish
        Color::linear_rgb(1.0, 1.0, 1.0),
        Color::linear_rgb(0.6, 0.2, 0.8), // purple-ish
        Color::linear_rgb(1.0, 0.2, 1.0), // fuchsia-ish
    ];

    for (e, mut tf, mut sprite, mut star) in &mut q {
        // Integrate position and rotation
        tf.translation.x += star.vel.x * dt;
        tf.translation.y += star.vel.y * dt;
        star.angle += star.ang_vel * dt;

        // Gravity and simple damping
        star.vel.y -= star.gravity * dt;
        let damp = (1.0 - (star.damping * dt)).max(0.0);
        star.vel *= damp;

        // Drift down slightly like the 3D version
        tf.translation.y -= dt * 2.0;

        // Wobble based on position and time
        let dot = tf.translation.x * tf.translation.x + tf.translation.y * tf.translation.y;
        let wobble = (dot + t * 30.0).sin() * 20.0 * dt;
        tf.translation.x += wobble;
        tf.translation.y += wobble;

        // Apply rotation and scale
        tf.rotation = Quat::from_rotation_z(star.angle);
        star.size -= dt * 5.0;
        tf.scale = Vec3::splat(star.size.max(0.0));

        // Blink color from palette with index offset
        let idx = (((t * 15.0) as i32 + star.blit_index) % palette.len() as i32) as usize;
        // Slight fade as it shrinks
        let alpha = (star.size / 3.0).clamp(0.0, 1.0);
        sprite.color = palette[idx].with_alpha(alpha);

        if star.size <= 0.0 {
            commands.entity(e).despawn();
        }
    }
}
