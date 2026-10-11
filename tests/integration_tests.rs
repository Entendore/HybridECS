use hybrid_ecs::*;

#[derive(Debug, PartialEq, Clone)]
struct Position { x: f32, y: f32 }
#[derive(Debug, PartialEq, Clone)]
struct Velocity { x: f32, y: f32 }
#[derive(Debug, PartialEq, Clone)]
struct Health(i32);

#[test]
fn test_entity_lifecycle() {
    let mut world = World::new();
    let e1 = world.spawn((Position { x: 0.0, y: 0.0 },));
    assert!(world.is_alive(e1));
    world.despawn(e1);
    assert!(!world.is_alive(e1));
    let e2 = world.spawn((Position { x: 1.0, y: 1.0 },));
    assert_eq!(e2.id(), e1.id());
    assert_ne!(e2.generation(), e1.generation());
}

#[test]
fn test_query_and_iter() {
    let mut world = World::new();
    world.spawn((Position { x: 0.0, y: 0.0 }, Velocity { x: 1.0, y: 0.0 }));
    world.spawn((Position { x: 10.0, y: 10.0 }, Velocity { x: 0.0, y: 1.0 }));
    world.spawn((Position { x: 20.0, y: 20.0 },));

    let count = world.query::<(&Position, &Velocity), ()>().count();
    assert_eq!(count, 2);
    
    let pos_count = world.query::<&Position, ()>().count();
    assert_eq!(pos_count, 3);
}

#[test]
fn test_commands_and_scope() {
    let mut world = World::new();
    world.commands_scope(|commands| {
        let e = commands.spawn_empty();
        commands.entity(e).insert(Position { x: 10.0, y: 0.0 });
    });
    assert_eq!(world.query::<&Position, ()>().count(), 1);
}

#[test]
fn test_state_hooks() {
    enum GameState { MainMenu, InGame }
    
    let mut app = App::new();
    app.add_state(GameState::MainMenu);
    app.on_enter::<GameState>(|world| { world.spawn((Position { x: 99.0, y: 99.0 },)); });
    
    app.run();
    assert_eq!(app.world.query::<&Position, ()>().count(), 1);
}

#[test]
fn test_resource_hooks() {
    #[derive(Resource, Default)] struct RenderSettings;
    #[derive(Resource, Default)] struct Counter(u32);
    
    let mut world = World::new();
    world.insert_resource(Counter::default());
    world.register_resource_hooks::<RenderSettings>(|hooks| {
        hooks.on_insert = Some(Box::new(|w| { w.get_resource_mut::<Counter>().unwrap().0 += 1; }));
    });
    
    world.insert_resource(RenderSettings);
    assert_eq!(world.get_resource::<Counter>().unwrap().0, 1);
}

#[test]
fn test_spatial_query() {
    let mut world = World::new();
    let _e1 = world.spawn((Aabb { min: [0.0, 0.0], max: [5.0, 5.0] }, Position { x: 0.0, y: 0.0 }));
    let _e2 = world.spawn((Aabb { min: [10.0, 10.0], max: [15.0, 15.0] }, Position { x: 10.0, y: 10.0 }));
    
    broad_phase_system(&mut world);
    let mut qs = world.query_state::<&mut Position, With<Aabb>>();
    let mut sq = SpatialQuery::new(&mut qs, &mut world);
    
    let results = sq.iter_mut(&Aabb { min: [0.0, 0.0], max: [5.0, 5.0] });
    assert_eq!(results.len(), 1);
}
