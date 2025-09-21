use bevy::prelude::*;
use std::collections::HashMap;

use crate::{GameState, GameSet};
use crate::loading::TextureAssets;

const BUTTON_SIZE: f32 = 64.0;

pub struct CombatPlugin;

impl Plugin for CombatPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<AbilityBook>()
            .init_resource::<CombatState>()
            .init_resource::<EnemyTimeline>()
            .add_event::<HudShakeEvent>()
            .add_event::<DamageEvent>()
            .add_event::<ApplyDotEvent>()
            .add_event::<ButtonFlashEvent>()
            .add_systems(OnEnter(GameState::Playing), (spawn_hud, reset_combat))
            .add_systems(
                PreUpdate,
                (
                    tick_combat_timers,
                    process_cast_completion,
                    handle_ability_input,
                    process_buffered_ability,
                    process_gcd_queue,
                )
                    .chain()
                    .in_set(GameSet::InputApply)
                    .run_if(in_state(GameState::Playing)),
            )
            .add_systems(
                Update,
                run_enemy_timeline
                    .in_set(GameSet::Sim)
                    .run_if(in_state(GameState::Playing)),
            )
            .add_systems(
                Update,
                (
                    update_cooldown_bars,
                    update_cast_bar,
                    update_status_row,
                    update_muddled_layout,
                    update_muddled_buttons,
                    trigger_button_flash,
                    decay_button_shake,
                    animate_ui_effects,
                    apply_hud_shake,
                    shake_hud_node,
                )
                    .in_set(GameSet::Ui)
                    .run_if(in_state(GameState::Playing)),
            );
    }
}

// ==== Abilities and core combat state ====

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AbilityId {
    Strike,     // GCD instant
    Fireball,   // GCD hard cast
    WeaveDash,  // oGCD instant
    WeaveSong,  // oGCD instant
    Cleanse,    // oGCD instant - clears muddled
    Burn,       // GCD instant DoT (placeholder)
    Heal,       // GCD hard cast heal (placeholder)
    Swiftcast,  // oGCD buff: next cast instant within 10s
    Raging,     // oGCD buff window (placeholder)
    Jump,       // oGCD instant
}

#[derive(Debug, Clone)]
pub struct Ability {
    pub id: AbilityId,
    pub name: &'static str,
    pub triggers_gcd: bool,
    pub cast_time: f32,   // seconds; 0.0 means instant
    pub cooldown: f32,    // seconds per ability
    pub ani_lock: f32,    // seconds the animation lock lasts
}

#[derive(Resource)]
pub struct AbilityBook {
    pub by_id: HashMap<AbilityId, Ability>,
}

impl Default for AbilityBook {
    fn default() -> Self {
        let mut by_id = HashMap::new();
        by_id.insert(
            AbilityId::Strike,
            Ability { id: AbilityId::Strike, name: "Strike", triggers_gcd: true, cast_time: 0.0, cooldown: 2.5, ani_lock: 0.6 },
        );
        by_id.insert(
            AbilityId::Fireball,
            Ability { id: AbilityId::Fireball, name: "Fireball", triggers_gcd: true, cast_time: 1.5, cooldown: 2.5, ani_lock: 0.6 },
        );
        by_id.insert(
            AbilityId::WeaveDash,
            Ability { id: AbilityId::WeaveDash, name: "Weave: Dash", triggers_gcd: false, cast_time: 0.0, cooldown: 20.0, ani_lock: 0.6 },
        );
        by_id.insert(
            AbilityId::WeaveSong,
            Ability { id: AbilityId::WeaveSong, name: "Weave: Song", triggers_gcd: false, cast_time: 0.0, cooldown: 30.0, ani_lock: 0.6 },
        );
        by_id.insert(
            AbilityId::Cleanse,
            Ability { id: AbilityId::Cleanse, name: "Cleanse", triggers_gcd: false, cast_time: 0.0, cooldown: 12.0, ani_lock: 0.1 },
        );
        by_id.insert(
            AbilityId::Burn,
            Ability { id: AbilityId::Burn, name: "Burn", triggers_gcd: true, cast_time: 0.0, cooldown: 2.5, ani_lock: 0.6 },
        );
        by_id.insert(
            AbilityId::Heal,
            Ability { id: AbilityId::Heal, name: "Heal", triggers_gcd: true, cast_time: 2.0, cooldown: 2.5, ani_lock: 0.6 },
        );
        by_id.insert(
            AbilityId::Swiftcast,
            Ability { id: AbilityId::Swiftcast, name: "Swiftcast", triggers_gcd: false, cast_time: 0.0, cooldown: 60.0, ani_lock: 0.6 },
        );
        by_id.insert(
            AbilityId::Raging,
            Ability { id: AbilityId::Raging, name: "Raging", triggers_gcd: false, cast_time: 0.0, cooldown: 90.0, ani_lock: 0.6 },
        );
        by_id.insert(
            AbilityId::Jump,
            Ability { id: AbilityId::Jump, name: "Jump", triggers_gcd: false, cast_time: 0.0, cooldown: 30.0, ani_lock: 0.6 },
        );
        Self { by_id }
    }
}

#[derive(Debug, Clone)]
pub struct CastState {
    pub ability: AbilityId,
    pub remaining: f32,
    pub total: f32,
}

#[derive(Debug, Resource)]
pub struct CombatState {
    pub gcd_remaining: f32,
    pub weaves_in_current_gcd: u8,
    pub cast: Option<CastState>,
    pub buffer: Option<(AbilityId, f32)>, // (ability, time_left)
    pub gcd_queue: Option<AbilityId>,     // queued next GCD
    pub ability_cds: HashMap<AbilityId, f32>,
    pub gcd_length: f32,
    pub buffer_window: f32,
    pub clipped: bool,
    pub muddled: Option<f32>, // time remaining
    pub hud_shake_remaining: f32,
    pub ani_lock_remaining: f32,
    pub gcd_queue_window: f32,
    pub swiftcast_remaining: Option<f32>, // seconds left to use; next cast instant
    pub raging_remaining: Option<f32>,    // placeholder buff
}

impl CombatState {
    fn can_use_now(&self, ability: &Ability) -> bool {
        let cd_ready = self
            .ability_cds
            .get(&ability.id)
            .copied()
            .unwrap_or(0.0)
            .le(&0.0);
        let not_casting = self.cast.is_none();
        if ability.triggers_gcd {
            // Next GCD can start only when GCD ready and no animation lock
            cd_ready && not_casting && self.gcd_remaining <= 0.0 && self.ani_lock_remaining <= 0.0
        } else {
            // Weave only between GCDs, when not casting and not locked
            cd_ready
                && not_casting
                && self.gcd_remaining > 0.0
                && self.ani_lock_remaining <= 0.0
                && self.weaves_in_current_gcd < 2
        }
    }
}

impl Default for CombatState {
    fn default() -> Self {
        Self {
            gcd_remaining: 0.0,
            weaves_in_current_gcd: 0,
            cast: None,
            buffer: None,
            gcd_queue: None,
            ability_cds: HashMap::new(),
            gcd_length: 2.5,
            buffer_window: 0.6,
            clipped: false,
            muddled: None,
            hud_shake_remaining: 0.0,
            ani_lock_remaining: 0.0,
            gcd_queue_window: 0.6,
            swiftcast_remaining: None,
            raging_remaining: None,
        }
    }
}

// ==== HUD ==== 

#[derive(Component)]
struct HudRoot;

#[derive(Component)]
struct HotbarRoot {
    row: u8,
}

#[derive(Component)]
struct ButtonRow(u8);

#[derive(Component)]
struct ButtonContent;

#[derive(Component)]
struct AbilityButton {
    id: AbilityId,
    index: usize,
}

#[derive(Component)]
struct CooldownBar {
    id: AbilityId,
    triggers_gcd: bool,
}

#[derive(Component)]
struct CastBarRoot;

#[derive(Component)]
struct CastBarFill;

#[derive(Component)]
struct StatusRow;

fn spawn_hud(mut commands: Commands) {
    commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                position_type: PositionType::Relative,
                ..default()
            },
            HudRoot,
        ))
        .with_children(|root| {
            // Hotbar row 1 (1..5)
            // Hotbar row 1 (1..5)
            root
                .spawn((
                    Node {
                        width: Val::Auto,
                        height: Val::Px(80.0),
                        position_type: PositionType::Absolute,
                        bottom: Val::Px(10.0),
                        left: Val::Px(400.0), // approximate center baseline
                        flex_direction: FlexDirection::Row,
                        align_items: AlignItems::Center,
                        justify_content: JustifyContent::Center,
                        column_gap: Val::Px(8.0),
                        ..default()
                    },
                    HotbarRoot { row: 0 },
                ))
                .with_children(|hotbar| {
                    let ids = [
                        AbilityId::Strike,
                        AbilityId::Fireball,
                        AbilityId::WeaveDash,
                        AbilityId::WeaveSong,
                        AbilityId::Cleanse,
                    ];
                    for (i, id) in ids.into_iter().enumerate() {
                        hotbar
                            .spawn((
                                Button,
                                Node {
                                    width: Val::Px(BUTTON_SIZE),
                                    height: Val::Px(BUTTON_SIZE),
                                    justify_content: JustifyContent::Center,
                                    align_items: AlignItems::Center,
                                    position_type: PositionType::Relative,
                                    ..default()
                                },
                                BackgroundColor(Color::linear_rgb(0.1, 0.1, 0.1)),
                                ButtonRow(0),
                            ))
                            .with_children(|btn| {
                                // Inner content that we offset for shake/muddle without shifting layout
                                btn
                                    .spawn((
                                        Node {
                                            width: Val::Percent(100.0),
                                            height: Val::Percent(100.0),
                                            position_type: PositionType::Absolute,
                                            left: Val::Px(0.0),
                                            bottom: Val::Px(0.0),
                                            ..default()
                                        },
                                        ButtonContent,
                                        AbilityButton { id, index: i },
                                    ))
                                    .with_children(|content| {
                                        content.spawn((
                                            Node {
                                                width: Val::Percent(100.0),
                                                height: Val::Px(0.0),
                                                position_type: PositionType::Absolute,
                                                left: Val::Px(0.0),
                                                right: Val::Px(0.0),
                                                bottom: Val::Px(0.0),
                                                ..default()
                                            },
                                            BackgroundColor(Color::linear_rgb(0.0, 0.0, 0.0).with_alpha(0.75)),
                                            CooldownBar { id, triggers_gcd: matches!(id, AbilityId::Strike | AbilityId::Fireball) },
                                        ));
                                        content.spawn((
                                            Text::new(format!("{}", i + 1)),
                                            TextFont { font_size: 18.0, ..default() },
                                            TextColor(Color::WHITE),
                                        ));
                                    });
                            });
                    }
                });

            // Hotbar row 2 (6..0)
            root
                .spawn((
                    Node {
                        width: Val::Auto,
                        height: Val::Px(80.0),
                        position_type: PositionType::Absolute,
                        bottom: Val::Px(90.0),
                        left: Val::Px(400.0),
                        flex_direction: FlexDirection::Row,
                        align_items: AlignItems::Center,
                        justify_content: JustifyContent::Center,
                        column_gap: Val::Px(8.0),
                        ..default()
                    },
                    HotbarRoot { row: 1 },
                ))
                .with_children(|hotbar| {
                    let ids = [
                        AbilityId::Burn,      // 6
                        AbilityId::Heal,      // 7
                        AbilityId::Swiftcast, // 8
                        AbilityId::Raging,    // 9
                        AbilityId::Jump,      // 0
                    ];
                    let labels = ["6", "7", "8", "9", "0"];
                    for (i, (id, label)) in ids.into_iter().zip(labels).enumerate() {
                        hotbar
                            .spawn((
                                Button,
                                Node {
                                    width: Val::Px(BUTTON_SIZE),
                                    height: Val::Px(BUTTON_SIZE),
                                    justify_content: JustifyContent::Center,
                                    align_items: AlignItems::Center,
                                    position_type: PositionType::Relative,
                                    ..default()
                                },
                                BackgroundColor(Color::linear_rgb(0.1, 0.1, 0.1)),
                                ButtonRow(1),
                            ))
                            .with_children(|btn| {
                                // Inner content wrapper
                                btn
                                    .spawn((
                                        Node {
                                            width: Val::Percent(100.0),
                                            height: Val::Percent(100.0),
                                            position_type: PositionType::Absolute,
                                            left: Val::Px(0.0),
                                            bottom: Val::Px(0.0),
                                            ..default()
                                        },
                                        ButtonContent,
                                        AbilityButton { id, index: i + 5 },
                                    ))
                                    .with_children(|content| {
                                        content.spawn((
                                            Node {
                                                width: Val::Percent(100.0),
                                                height: Val::Px(0.0),
                                                position_type: PositionType::Absolute,
                                                left: Val::Px(0.0),
                                                right: Val::Px(0.0),
                                                bottom: Val::Px(0.0),
                                                ..default()
                                            },
                                            BackgroundColor(Color::linear_rgb(0.0, 0.0, 0.0).with_alpha(0.75)),
                                            CooldownBar { id, triggers_gcd: matches!(id, AbilityId::Burn | AbilityId::Heal) },
                                        ));
                                        content.spawn((
                                            Text::new(label.to_string()),
                                            TextFont { font_size: 18.0, ..default() },
                                            TextColor(Color::WHITE),
                                        ));
                                    });
                            });
                    }
                });

            // Cast bar
            root
                .spawn((
                    Node {
                        width: Val::Px(400.0),
                        height: Val::Px(18.0),
                        position_type: PositionType::Absolute,
                        bottom: Val::Px(100.0),
                        left: Val::Percent(50.0),
                        justify_content: JustifyContent::FlexStart,
                        align_items: AlignItems::Stretch,
                        ..default()
                    },
                    BackgroundColor(Color::linear_rgb(0.05, 0.05, 0.05)),
                    CastBarRoot,
                ))
                .with_children(|bar| {
                    bar.spawn((
                        Node { width: Val::Percent(0.0), height: Val::Percent(100.0), ..default() },
                        BackgroundColor(Color::linear_rgb(0.2, 0.6, 1.0)),
                        CastBarFill,
                    ));
                });

            // Status row
            root.spawn((
                Node {
                    width: Val::Auto,
                    height: Val::Px(24.0),
                    position_type: PositionType::Absolute,
                    top: Val::Px(10.0),
                    left: Val::Px(10.0),
                    align_items: AlignItems::Center,
                    column_gap: Val::Px(8.0),
                    ..default()
                },
                StatusRow,
            ));
        });
}

fn reset_combat(mut combat: ResMut<CombatState>) {
    *combat = CombatState::default();
}

// ==== Input and execution ====

fn handle_ability_input(
    keys: Res<ButtonInput<KeyCode>>,
    book: Res<AbilityBook>,
    mut combat: ResMut<CombatState>,
    mut dmg_writer: EventWriter<DamageEvent>,
    mut dot_writer: EventWriter<ApplyDotEvent>,
    mut flash_writer: EventWriter<ButtonFlashEvent>,
) {
    let mapping: &[(KeyCode, AbilityId)] = &[
        (KeyCode::Digit1, AbilityId::Strike),
        (KeyCode::Digit2, AbilityId::Fireball),
        (KeyCode::Digit3, AbilityId::WeaveDash),
        (KeyCode::Digit4, AbilityId::WeaveSong),
        (KeyCode::Digit5, AbilityId::Cleanse),
        (KeyCode::Digit6, AbilityId::Burn),
        (KeyCode::Digit7, AbilityId::Heal),
        (KeyCode::Digit8, AbilityId::Swiftcast),
        (KeyCode::Digit9, AbilityId::Raging),
        (KeyCode::Digit0, AbilityId::Jump),
    ];

    for (kc, id) in mapping.iter().copied() {
        if keys.just_pressed(kc) {
            if let Some(ability) = book.by_id.get(&id) {
                flash_writer.write(ButtonFlashEvent { id });
                try_use_or_buffer(ability, &mut combat, &mut dmg_writer, &mut dot_writer);
            }
        }
    }
}

fn try_use_or_buffer(
    ability: &Ability,
    combat: &mut CombatState,
    dmg_writer: &mut EventWriter<DamageEvent>,
    dot_writer: &mut EventWriter<ApplyDotEvent>,
) {
    if combat.can_use_now(ability) {
        start_cast_or_instant(ability, combat, dmg_writer, dot_writer);
        return;
    }
    if ability.triggers_gcd {
        // Queue next GCD near the end of current GCD or cast
        if let Some(cast) = &combat.cast {
            if cast.remaining <= combat.gcd_queue_window {
                combat.gcd_queue = Some(ability.id);
                return;
            }
        }
        if combat.gcd_remaining > 0.0 && combat.gcd_remaining <= combat.gcd_queue_window {
            combat.gcd_queue = Some(ability.id);
            return;
        }
    }
    // Fallback short buffer
    combat.buffer = Some((ability.id, combat.buffer_window.min(0.4)));
}

fn start_cast_or_instant(
    ability: &Ability,
    combat: &mut CombatState,
    dmg_writer: &mut EventWriter<DamageEvent>,
    dot_writer: &mut EventWriter<ApplyDotEvent>,
) {
    let mut cast_time = ability.cast_time;
    // Swiftcast makes next cast instant
    if cast_time > 0.0 {
        if let Some(rem) = combat.swiftcast_remaining {
            if rem > 0.0 {
                cast_time = 0.0;
                combat.swiftcast_remaining = None; // consume buff
            }
        }
    }
    if cast_time > 0.0 {
        combat.cast = Some(CastState { ability: ability.id, remaining: cast_time, total: cast_time });
    } else {
        resolve_ability(ability, combat, dmg_writer, dot_writer);
    }
}

fn resolve_ability(
    ability: &Ability,
    combat: &mut CombatState,
    dmg_writer: &mut EventWriter<DamageEvent>,
    dot_writer: &mut EventWriter<ApplyDotEvent>,
) {
    // Apply cooldown
    combat.ability_cds.insert(ability.id, ability.cooldown);

    if ability.triggers_gcd {
        // Start/refresh GCD
        combat.gcd_remaining = combat.gcd_length;
        combat.weaves_in_current_gcd = 0;
        combat.clipped = false;
        combat.ani_lock_remaining = ability.ani_lock;
        // Instant damage for GCD if any
        let mult = if combat.raging_remaining.unwrap_or(0.0) > 0.0 { 1.2 } else { 1.0 };
        let base = match ability.id { AbilityId::Strike => 100, AbilityId::Fireball => 180, AbilityId::Burn => 0, AbilityId::Heal => 0, _ => 0 };
        if base > 0 { dmg_writer.write(DamageEvent { amount: ((base as f32) * mult) as i32 }); }
        if ability.id == AbilityId::Burn { dot_writer.write(ApplyDotEvent { dps: 20, duration: 12.0, tick_every: 1.0 }); }
    } else {
        // oGCD weave window logic
        combat.weaves_in_current_gcd = combat.weaves_in_current_gcd.saturating_add(1);
        if combat.weaves_in_current_gcd > 2 { combat.clipped = true; }
        combat.ani_lock_remaining = ability.ani_lock;

        // Special abilities
        match ability.id {
            AbilityId::Cleanse => { combat.muddled = None; }
            AbilityId::Swiftcast => { combat.swiftcast_remaining = Some(10.0); }
            AbilityId::Raging => { combat.raging_remaining = Some(15.0); }
            AbilityId::WeaveDash => { dmg_writer.write(DamageEvent { amount: 60 }); }
            AbilityId::WeaveSong => { dmg_writer.write(DamageEvent { amount: 50 }); }
            AbilityId::Jump => { dmg_writer.write(DamageEvent { amount: 120 }); }
            _ => {}
        }
    }
}

fn tick_combat_timers(time: Res<Time>, mut combat: ResMut<CombatState>) {
    let dt = time.delta_secs();
    combat.gcd_remaining = (combat.gcd_remaining - dt).max(0.0);
    if let Some(cast) = &mut combat.cast {
        cast.remaining -= dt;
        if cast.remaining < 0.0 {
            cast.remaining = 0.0;
        }
    }
    for v in combat.ability_cds.values_mut() {
        *v = (*v - dt).max(0.0);
    }
    if let Some((id, left)) = combat.buffer.take() {
        let new_left = left - dt;
        if new_left > 0.0 { combat.buffer = Some((id, new_left)); }
    }
    if let Some(t) = combat.muddled.as_mut() { *t = (*t - dt).max(0.0); if *t == 0.0 { combat.muddled = None; } }
    if combat.hud_shake_remaining > 0.0 { combat.hud_shake_remaining = (combat.hud_shake_remaining - dt).max(0.0); }
    if combat.ani_lock_remaining > 0.0 { combat.ani_lock_remaining = (combat.ani_lock_remaining - dt).max(0.0); }
    if let Some(t) = combat.swiftcast_remaining.as_mut() { *t = (*t - dt).max(0.0); if *t == 0.0 { combat.swiftcast_remaining = None; } }
    if let Some(t) = combat.raging_remaining.as_mut() { *t = (*t - dt).max(0.0); if *t == 0.0 { combat.raging_remaining = None; } }
}

fn process_cast_completion(
    book: Res<AbilityBook>,
    mut combat: ResMut<CombatState>,
    mut dmg_writer: EventWriter<DamageEvent>,
    mut dot_writer: EventWriter<ApplyDotEvent>,
) {
    if let Some(cast) = &combat.cast {
        if cast.remaining <= 0.0 {
            if let Some(ability) = book.by_id.get(&cast.ability) {
                resolve_ability(ability, &mut combat, &mut dmg_writer, &mut dot_writer);
            }
            combat.cast = None;
        }
    }
}

fn process_buffered_ability(
    book: Res<AbilityBook>,
    mut combat: ResMut<CombatState>,
    mut dmg_writer: EventWriter<DamageEvent>,
    mut dot_writer: EventWriter<ApplyDotEvent>,
) {
    if combat.cast.is_some() { return; }
    if let Some((id, _)) = combat.buffer {
        if let Some(ability) = book.by_id.get(&id) {
            if combat.can_use_now(ability) {
                combat.buffer = None;
                start_cast_or_instant(ability, &mut combat, &mut dmg_writer, &mut dot_writer);
            }
        }
    }
}

fn process_gcd_queue(
    book: Res<AbilityBook>,
    mut combat: ResMut<CombatState>,
    mut dmg_writer: EventWriter<DamageEvent>,
    mut dot_writer: EventWriter<ApplyDotEvent>,
) {
    if combat.cast.is_some() { return; }
    if let Some(id) = combat.gcd_queue {
        if let Some(ability) = book.by_id.get(&id) {
            if ability.triggers_gcd && combat.gcd_remaining <= 0.0 && combat.ani_lock_remaining <= 0.0 {
                combat.gcd_queue = None;
                start_cast_or_instant(ability, &mut combat, &mut dmg_writer, &mut dot_writer);
            }
        }
    }
}

// ==== HUD updates ====

fn update_cooldown_bars(
    book: Res<AbilityBook>,
    combat: Res<CombatState>,
    mut q: Query<(&CooldownBar, &mut Node, &mut BackgroundColor)>,
) {
    for (bar, mut node, mut color) in &mut q {
        let cd = combat.ability_cds.get(&bar.id).copied().unwrap_or(0.0);
        let total = book.by_id.get(&bar.id).map(|a| a.cooldown).unwrap_or(1.0);
        let mut frac_cd = if total > 0.0 { (cd / total).clamp(0.0, 1.0) } else { 0.0 };
        let mut frac_gcd = 0.0;
        if bar.triggers_gcd {
            if combat.gcd_length > 0.0 {
                frac_gcd = (combat.gcd_remaining / combat.gcd_length).clamp(0.0, 1.0);
            }
        }
        let frac = frac_cd.max(frac_gcd);
        let px = (BUTTON_SIZE * frac).floor().max(0.0);
        node.height = Val::Px(px);
        // if bar is due to gcd, tint bluish, else gray; if both, bluish wins
        if frac_gcd > frac_cd { color.0 = Color::linear_rgb(0.2, 0.4, 0.9).with_alpha(0.75); }
        else { color.0 = Color::linear_rgb(0.0, 0.0, 0.0).with_alpha(0.75); }
    }
}

fn update_cast_bar(
    combat: Res<CombatState>,
    mut q_fill: Query<&mut Node, With<CastBarFill>>,
) {
    if let Ok(mut node) = q_fill.single_mut() {
        if let Some(cast) = &combat.cast {
            let p = if cast.total > 0.0 { 1.0 - (cast.remaining / cast.total) } else { 1.0 };
            node.width = Val::Percent((p * 100.0).clamp(0.0, 100.0));
        } else {
            node.width = Val::Percent(0.0);
        }
    }
}

fn update_status_row(
    mut commands: Commands,
    combat: Res<CombatState>,
    row: Query<Entity, With<StatusRow>>,
    q_children: Query<&Children>,
) {
    let Ok(row_entity) = row.single() else { return; };
    // Despawn existing direct children (they are leaf Text nodes)
    if let Ok(children) = q_children.get(row_entity) {
        for child in children.iter() {
            commands.entity(child).despawn();
        }
    }
    commands.entity(row_entity).with_children(|r| {
        if combat.muddled.is_some() {
            r.spawn((Text::new("Muddled"), TextFont { font_size: 16.0, ..default() }, TextColor(Color::linear_rgb(1.0, 0.3, 0.2))));
        }
        if combat.hud_shake_remaining > 0.0 {
            r.spawn((Text::new("HUD Shaking"), TextFont { font_size: 16.0, ..default() }, TextColor(Color::linear_rgb(0.95, 0.9, 0.2))));
        }
        if let Some(t) = combat.swiftcast_remaining { if t > 0.0 { r.spawn((Text::new("Swiftcast"), TextFont { font_size: 16.0, ..default() }, TextColor(Color::linear_rgb(0.5, 0.9, 1.0)))); } }
        if let Some(t) = combat.raging_remaining { if t > 0.0 { r.spawn((Text::new("Raging"), TextFont { font_size: 16.0, ..default() }, TextColor(Color::linear_rgb(1.0, 0.5, 0.2)))); } }
    });
}

fn update_muddled_layout(
    _time: Res<Time>,
    _combat: Res<CombatState>,
    mut q_hotbars: Query<(&mut Node, &HotbarRoot)>,
) {
    // Keep rows anchored; per-button drift is handled in update_muddled_buttons
    for (mut node, root) in &mut q_hotbars {
        let base_left = 400.0;
        let base_bottom = if root.row == 0 { 10.0 } else { 90.0 };
        node.left = Val::Px(base_left);
        node.bottom = Val::Px(base_bottom);
    }
}

fn update_muddled_buttons(
    time: Res<Time>,
    combat: Res<CombatState>,
    mut q_buttons: Query<(&AbilityButton, &ChildOf, &mut Node, Option<&ButtonShake>)>,
    q_button_rows: Query<&ButtonRow>,
) {
    for (btn, parent, mut node, shake) in &mut q_buttons {
        if combat.muddled.is_some() {
            let row = q_button_rows
                .get(parent.parent())
                .map(|r| r.0)
                .unwrap_or(0);
            let t = time.elapsed_secs();
            // unique phase per button using index and row
            let phase = (btn.index as f32) * 0.8 + (row as f32) * 0.5;
            let dx = (t * 1.9 + phase).sin() * 24.0;
            let dy = (t * 1.3 + phase).cos() * 12.0;
            let (sx, sy) = if let Some(sh) = shake {
                if sh.remaining > 0.0 {
                    let tt = time.elapsed_secs() + sh.phase;
                    let amp = sh.amp * (sh.remaining.min(1.0));
                    let sdx = (tt * sh.freq).sin() * amp;
                    let sdy = (tt * sh.freq * 0.9).cos() * amp * 0.5;
                    (sdx, sdy)
                } else { (0.0, 0.0) }
            } else { (0.0, 0.0) };
            node.left = Val::Px(dx + sx);
            node.bottom = Val::Px(dy + sy);
        } else {
            let (sx, sy) = if let Some(sh) = shake {
                if sh.remaining > 0.0 {
                    let tt = time.elapsed_secs() + sh.phase;
                    let amp = sh.amp * (sh.remaining.min(1.0));
                    let sdx = (tt * sh.freq).sin() * amp;
                    let sdy = (tt * sh.freq * 0.9).cos() * amp * 0.5;
                    (sdx, sdy)
                } else { (0.0, 0.0) }
            } else { (0.0, 0.0) };
            node.left = Val::Px(sx);
            node.bottom = Val::Px(sy);
        }
    }
}

// ==== Enemy timeline and effects ====

#[derive(Debug, Clone)]
enum EnemyEvent {
    Muddled { duration: f32 },
    HudShake { duration: f32 },
    Enrage,
}

#[derive(Resource, Default)]
struct EnemyTimeline {
    t: f32,
    idx: usize,
    events: Vec<(f32, EnemyEvent)>,
}

impl EnemyTimeline {
    fn ensure_default_events(&mut self) {
        if self.events.is_empty() {
            self.events = vec![
                (3.0, EnemyEvent::HudShake { duration: 1.0 }),
                (6.0, EnemyEvent::Muddled { duration: 8.0 }),
                (15.0, EnemyEvent::HudShake { duration: 1.5 }),
                (25.0, EnemyEvent::Enrage),
            ];
            self.t = 0.0;
            self.idx = 0;
        }
    }
}

#[derive(Event)]
struct HudShakeEvent(pub f32);

#[derive(Event, Debug, Clone, Copy)]
pub struct DamageEvent {
    pub amount: i32,
}

#[derive(Event, Debug, Clone, Copy)]
pub struct ApplyDotEvent {
    pub dps: i32,
    pub duration: f32,
    pub tick_every: f32,
}

#[derive(Event, Debug, Clone, Copy)]
struct ButtonFlashEvent {
    id: AbilityId,
}

#[derive(Component)]
struct ButtonShake {
    remaining: f32,
    amp: f32,
    freq: f32,
    phase: f32,
}

#[derive(Component)]
struct UiOneShotEffect {
    ttl: f32,
    total: f32,
    base_px: f32,
}

fn run_enemy_timeline(
    time: Res<Time>,
    mut timeline: ResMut<EnemyTimeline>,
    mut combat: ResMut<CombatState>,
    mut shake_writer: EventWriter<HudShakeEvent>,
) {
    timeline.ensure_default_events();
    timeline.t += time.delta_secs();
    while timeline.idx < timeline.events.len() && timeline.t >= timeline.events[timeline.idx].0 {
        let event = timeline.events[timeline.idx].1.clone();
        match event {
            EnemyEvent::Muddled { duration } => {
                combat.muddled = Some(duration);
            }
            EnemyEvent::HudShake { duration } => {
                shake_writer.write(HudShakeEvent(duration));
            }
            EnemyEvent::Enrage => {
                // Simulate instant kill: brutal HUD shake and reset
                combat.hud_shake_remaining = 2.0;
                combat.muddled = Some(5.0);
                // Restart cycle
                timeline.t = 0.0;
                timeline.idx = 0;
                continue; // skip idx increment reset below
            }
        }
        timeline.idx += 1;
    }
}

fn apply_hud_shake(
    mut reader: EventReader<HudShakeEvent>,
    mut combat: ResMut<CombatState>,
) {
    for HudShakeEvent(dur) in reader.read() {
        combat.hud_shake_remaining = combat.hud_shake_remaining.max(*dur);
    }
}

fn shake_hud_node(
    time: Res<Time>,
    combat: Res<CombatState>,
    mut q: Query<&mut Node, With<HudRoot>>,
) {
    if let Ok(mut node) = q.single_mut() {
        if combat.hud_shake_remaining > 0.0 {
            let t = time.elapsed_secs();
            let amp = 6.0;
            let dx = (t * 30.0).sin() * amp;
            let dy = (t * 37.0).cos() * amp;
            node.left = Val::Px(dx);
            node.top = Val::Px(dy);
        } else {
            node.left = Val::Px(0.0);
            node.top = Val::Px(0.0);
        }
    }
}

fn trigger_button_flash(
    textures: Res<TextureAssets>,
    mut commands: Commands,
    mut evr: EventReader<ButtonFlashEvent>,
    q_buttons: Query<(Entity, &AbilityButton)>,
) {
    for ButtonFlashEvent { id } in evr.read() {
        if let Some((entity, _)) = q_buttons.iter().find(|(_, b)| b.id == *id) {
            // Insert/refresh shake
            commands.entity(entity).insert(ButtonShake {
                remaining: 1.0,
                amp: 6.0,
                freq: 40.0,
                phase: (*id as u32 % 7) as f32,
            });
            // Spawn one-shot effect bigger than the button
            let base = BUTTON_SIZE * 1.6;
            commands.entity(entity).with_children(|p| {
                p.spawn((
                    Node {
                        width: Val::Px(base),
                        height: Val::Px(base),
                        position_type: PositionType::Absolute,
                        left: Val::Px(-(base - BUTTON_SIZE) * 0.5),
                        bottom: Val::Px(-(base - BUTTON_SIZE) * 0.5),
                        ..default()
                    },
                    ImageNode { image: textures.bevy.clone(), ..default() },
                    UiOneShotEffect { ttl: 0.3, total: 0.3, base_px: base },
                ));
            });
        }
    }
}

fn decay_button_shake(time: Res<Time>, mut q: Query<&mut ButtonShake>) {
    let dt = time.delta_secs();
    for mut sh in &mut q {
        sh.remaining = (sh.remaining - dt).max(0.0);
    }
}

fn animate_ui_effects(time: Res<Time>, mut q: Query<(Entity, &mut Node, &mut UiOneShotEffect)>, mut commands: Commands) {
    let dt = time.delta_secs();
    for (e, mut node, mut fx) in &mut q {
        fx.ttl -= dt;
        let a = (fx.ttl / fx.total).clamp(0.0, 1.0);
        let size = fx.base_px * (0.7 + 0.3 * a);
        node.width = Val::Px(size);
        node.height = Val::Px(size);
        if fx.ttl <= 0.0 {
            commands.entity(e).despawn();
        }
    }
}
