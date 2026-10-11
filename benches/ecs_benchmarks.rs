use criterion::{black_box, criterion_group, criterion_main, Criterion};
use hybrid_ecs::*;

#[derive(Debug, PartialEq, Clone)]
struct Position { x: f32, y: f32 }
#[derive(Debug, PartialEq, Clone)]
struct Velocity { x: f32, y: f32 }

fn bench_spawn_100k(c: &mut Criterion) {
    c.bench_function("spawn_100k", |b| {
        b.iter(|| {
            let mut world = World::new();
            for i in 0..100_000 { world.spawn((Position { x: i as f32, y: 0.0 },)); }
            black_box(());
        });
    });
}

fn bench_iter_mut_100k(c: &mut Criterion) {
    let mut world = World::new();
    for i in 0..100_000 { world.spawn((Position { x: i as f32, y: 0.0 }, Velocity { x: 1.0, y: 0.0 })); }
    
    c.bench_function("iter_mut_100k", |b| {
        b.iter(|| {
            for (mut pos, _vel) in world.query_mut::<(&mut Position, &Velocity), ()>() { pos.x += 1.0; }
            black_box(());
        });
    });
}

criterion_group!(benches, bench_spawn_100k, bench_iter_mut_100k);
criterion_main!(benches);
