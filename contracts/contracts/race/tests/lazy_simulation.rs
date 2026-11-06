//! Fast deterministic accounting simulation; not a Wasm gas benchmark or a new engine.
use cosmwasm_std::{Addr, Uint128};
use racer_collection::{Attributes, Bonus, Kind, Rarity};
use racer_race::{
    math::{damage, make_player, move_player, rocket_price},
    state::{Boosts, Player},
    Parameters,
};
use std::time::Instant;

fn params() -> Parameters {
    Parameters {
        tick_seconds: 6,
        races_per_cycle: 100,
        duration_cdf: vec![1_000_000_000; 240],
        boost_price: Uint128::new(1_000_000),
        speed_boost: Uint128::new(10000),
        acceleration: Uint128::new(100),
        temporary_distance: Uint128::new(2_000_000),
        temporary_ticks: 50,
        rocket_cost_bps: 4000,
        rocket_damage_bps: 10000,
        boost_fee_bps: 500,
        maintenance_referral_bps: 500,
        max_actions_per_tick: 10,
        max_players: 1000,
    }
}

fn boosts(index: usize, tick: u32) -> Boosts {
    if index % 17 != 0 {
        return Boosts::default();
    }
    Boosts {
        speed: u32::from([7, 37].contains(&tick)),
        acceleration: u32::from([11, 57].contains(&tick)),
        temporary: u32::from([5, 95].contains(&tick)),
        ..Boosts::default()
    }
}

fn catch_up(player: &mut Player, last: &mut u32, index: usize, tick: u32, p: &Parameters) -> usize {
    if *last == tick {
        return 0;
    }
    for t in *last + 1..=tick {
        move_player(player, &boosts(index, t), p, t).unwrap();
    }
    *last = tick;
    1 // One resulting player persistence, not one write per replayed tick.
}

#[test]
fn rocket_price_is_twenty_point_nine_nine_srt_at_tier_one_hundred() {
    for (tier, expected) in [(1, 200_000), (20, 4_038_000), (100, 20_990_000)] {
        let player = make_player(1, Addr::unchecked("pilot"), [1, 1, tier], None).unwrap();
        assert_eq!(rocket_price(&player, &params()).u128(), expected);
        println!(
            "fighter level {tier}: {} SRT/rocket",
            expected as f64 / 1_000_000.0
        );
    }
}

#[test]
fn acceleration_accumulates_large_purchases_across_windows_without_u32_overflow() {
    let mut player = make_player(1, Addr::unchecked("pilot"), [1, 1, 1], None).unwrap();
    let boosts = Boosts {
        acceleration: u32::MAX,
        ..Boosts::default()
    };
    move_player(&mut player, &boosts, &params(), 1).unwrap();
    move_player(&mut player, &boosts, &params(), 2).unwrap();
    assert_eq!(player.acceleration_count, u64::from(u32::MAX) * 2);
}

#[test]
fn time_aware_tree_matches_eager_weights_and_exact_target_intervals_for_one_thousand_racers() {
    use cosmwasm_std::{
        testing::{mock_dependencies, mock_env},
        MessageInfo,
    };
    use racer_race::{
        motion,
        state::{SCALE, STATE},
        InstantiateMsg,
    };
    let mut deps = mock_dependencies();
    let owner = deps.api.addr_make("owner");
    let p = params();
    racer_race::instantiate(
        deps.as_mut(),
        mock_env(),
        MessageInfo {
            sender: owner.clone(),
            funds: vec![],
        },
        InstantiateMsg {
            owner: owner.to_string(),
            fleet: owner.to_string(),
            equipment: owner.to_string(),
            rewards: owner.to_string(),
            randomness: owner.to_string(),
            srt: owner.to_string(),
            treasury: owner.to_string(),
            parameters: p.clone(),
            resume_from: None,
        },
    )
    .unwrap();
    let mut state = STATE.load(&deps.storage).unwrap();
    state.race = 1;
    state.participants = 1000;
    let mut eager: Vec<_> = (0..1000)
        .map(|i| {
            make_player(
                1,
                Addr::unchecked(format!("pilot{i}")),
                [1 + i % 20, 1, 1],
                None,
            )
            .unwrap()
        })
        .collect();
    let mut stored = eager.clone();
    for (i, player) in stored.iter().enumerate() {
        motion::update(&mut deps.storage, 1, i as u16 + 1, player, &p).unwrap();
    }
    let mut reconciled = 0;
    for tick in 1..=70 {
        state.tick = tick;
        state.cursor = 0;
        for (i, player) in eager.iter_mut().enumerate() {
            let b = if i % 17 == 0 && [7, 9, 17].contains(&tick) {
                Boosts {
                    speed: 1000,
                    acceleration: 1000,
                    temporary: 1000,
                    ..Boosts::default()
                }
            } else {
                Boosts::default()
            };
            move_player(player, &b, &p, tick).unwrap();
            if b.count() > 0 {
                motion::WORK
                    .save(&mut deps.storage, (1, tick, i as u16 + 1), &true)
                    .unwrap();
            }
        }
        let indices: Vec<_> = motion::WORK
            .prefix((1, tick))
            .keys(&deps.storage, None, None, cosmwasm_std::Order::Ascending)
            .collect::<Result<_, _>>()
            .unwrap();
        for index in indices {
            let i = usize::from(index - 1);
            motion::catch_up(&mut stored[i], tick - 1, &p).unwrap();
            let b = if i % 17 == 0 && [7, 9, 17].contains(&tick) {
                Boosts {
                    speed: 1000,
                    acceleration: 1000,
                    temporary: 1000,
                    ..Boosts::default()
                }
            } else {
                Boosts::default()
            };
            move_player(&mut stored[i], &b, &p, tick).unwrap();
            stored[i].materialized_tick = tick;
            motion::update(&mut deps.storage, 1, index, &stored[i], &p).unwrap();
            motion::WORK.remove(&mut deps.storage, (1, tick, index));
            reconciled += 1;
        }
        if [15, 16, 30].contains(&tick) {
            for index in [1u16, 17, 321] {
                let i = usize::from(index - 1);
                motion::catch_up(&mut stored[i], tick, &p).unwrap();
                damage(&mut stored[i], Uint128::new(600 * SCALE)).unwrap();
                damage(&mut eager[i], Uint128::new(600 * SCALE)).unwrap();
                motion::update(&mut deps.storage, 1, index, &stored[i], &p).unwrap();
            }
        }
        let weights: Vec<_> = eager
            .iter()
            .map(|player| racer_race::math::weight(player).unwrap().u128())
            .collect();
        let total: u128 = weights.iter().sum();
        assert_eq!(
            motion::prefix(&deps.storage, &state, 16384).unwrap().u128(),
            total,
            "aggregate at tick {tick}"
        );
        for offset in [0, total / 3, total / 2, total - 1] {
            let mut sum = 0;
            let expected = weights
                .iter()
                .position(|weight| {
                    sum += weight;
                    offset < sum
                })
                .unwrap()
                + 1;
            assert_eq!(
                usize::from(motion::find(&deps.storage, &state, Uint128::new(offset)).unwrap()),
                expected,
                "target interval at tick {tick}"
            );
        }
    }
    assert!(
        reconciled < 2000,
        "unexpected fleet-wide reconciliation: {reconciled}"
    );
    println!("1000-racer aggregate and weighted-target intervals exact across 70 ticks; {reconciled} reconciliations instead of 70000 fleet writes");
}

#[test]
fn no_regeneration_replay_matches_every_tick_with_damage_boosts_expiry_and_death() {
    let started = Instant::now();
    let p = params();
    for count in [3, 20, 100, 200, 300] {
        let mut eager: Vec<_> = (0..count)
            .map(|i| {
                let commander = (i % 2 == 0).then(|| Attributes {
                    code: "scrap_milo".into(),
                    kind: Kind::Commander,
                    rarity: Rarity::Rare,
                    bonus: Bonus::Repair,
                    bonus_bps: 500,
                });
                let mut player = make_player(
                    1,
                    Addr::unchecked(format!("pilot{i}")),
                    [3 + (i % 7) as u16, 1 + (i % 3) as u16, 1],
                    commander,
                )
                .unwrap();
                damage(&mut player, Uint128::new(2_000_000)).unwrap();
                if i % 13 == 0 {
                    // Fighter destroyed: it must never regenerate from zero.
                    player.ships[2].hp = Uint128::zero();
                }
                if i % 29 == 0 {
                    // Fully destroyed fleets must remain dead.
                    for ship in &mut player.ships {
                        ship.hp = Uint128::zero();
                    }
                    player.alive = false;
                }
                player
            })
            .collect();
        let mut lazy = eager.clone();
        let mut last = vec![0; count];
        let mut writes = 0;
        for tick in 1..=240 {
            for (i, player) in eager.iter_mut().enumerate() {
                move_player(player, &boosts(i, tick), &p, tick).unwrap();
                let b = boosts(i, tick);
                if b.speed + b.acceleration + b.temporary > 0 {
                    writes += catch_up(&mut lazy[i], &mut last[i], i, tick, &p);
                    assert_eq!(*player, lazy[i], "utility activation: {i}, {tick}");
                }
            }
            // Fixed combat frames/targets isolate accounting from randomness.
            if [19, 20, 79, 149].contains(&tick) || tick == 240 {
                for i in 0..count {
                    writes += catch_up(&mut lazy[i], &mut last[i], i, tick, &p);
                    assert_eq!(eager[i], lazy[i], "pre-combat/end: {i}, {tick}");
                }
                if tick != 240 {
                    for shot in 0..6 {
                        let target = (tick as usize * 17 + shot * 13) % count;
                        damage(&mut eager[target], Uint128::new(2_000_000)).unwrap();
                        damage(&mut lazy[target], Uint128::new(2_000_000)).unwrap();
                        assert_eq!(eager[target], lazy[target]);
                        writes += 1;
                    }
                }
            }
        }
        assert_eq!(eager, lazy);
        for (i, player) in lazy.iter().enumerate() {
            assert!(player.temporary.is_empty());
            if !player.alive {
                assert!(player.ships.iter().all(|s| s.hp.is_zero()));
            }
            assert!(player.ships.iter().all(|s| s.hp <= s.base_hp));
            if i % 13 == 0 {
                assert!(player.ships[2].hp.is_zero());
            }
        }
        println!("{count} racers, 240 ticks, 4 combat frames: eager player movement writes={}, lazy persistence proxy={writes}; all final state and checkpoint comparisons exact", count * 240);
    }
    println!(
        "native simulation elapsed: {:?} (no chain/block waits; not gas measurements)",
        started.elapsed()
    );
}

#[test]
fn no_regeneration_closed_form_matches_tick_rounding_for_damaged_accelerating_fleets() {
    for seed in 0..100u64 {
        let mut eager = make_player(
            1,
            Addr::unchecked("pilot"),
            [
                1 + (seed % 100) as u16,
                1 + (seed % 20) as u16,
                1 + (seed % 50) as u16,
            ],
            None,
        )
        .unwrap();
        eager.acceleration_count = seed * 71;
        eager.ships[1].hp = if seed % 3 == 0 {
            Uint128::zero()
        } else {
            eager.ships[1].hp.multiply_ratio(seed % 99 + 1, 100u64)
        };
        eager.ships[2].hp = eager.ships[2].hp.multiply_ratio(seed % 97 + 1, 100u64);
        let hp = eager.ships.iter().map(|s| s.hp).collect::<Vec<_>>();
        let mut lazy = eager.clone();
        for tick in 1..=240 {
            move_player(&mut eager, &Boosts::default(), &params(), tick).unwrap();
        }
        eager.materialized_tick = 240;
        racer_race::motion::catch_up(&mut lazy, 240, &params()).unwrap();
        assert_eq!(eager, lazy, "rounding seed {seed}");
        assert_eq!(lazy.ships.iter().map(|s| s.hp).collect::<Vec<_>>(), hp);
    }
}
