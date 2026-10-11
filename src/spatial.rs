use crate::component::Component;
use crate::entity::Entity;
use crate::query::{QueryFilter, QueryMut, QueryState, With};
use crate::resource::Resource;
use crate::world::World;
use std::collections::HashMap;

#[derive(Component, Debug, Clone, Copy)]
pub struct Aabb {
    pub min: [f32; 2],
    pub max: [f32; 2],
}

#[derive(Resource, Default)]
pub struct PhysicsWorld {
    pub grid: HashMap<(i32, i32), Vec<Entity>>,
    pub cell_size: f32,
}

impl PhysicsWorld {
    pub fn new(cell_size: f32) -> Self { Self { grid: HashMap::new(), cell_size } }
    pub fn insert(&mut self, entity: Entity, aabb: &Aabb) {
        let min_x = (aabb.min[0] / self.cell_size).floor() as i32;
        let min_y = (aabb.min[1] / self.cell_size).floor() as i32;
        let max_x = (aabb.max[0] / self.cell_size).floor() as i32;
        let max_y = (aabb.max[1] / self.cell_size).floor() as i32;
        for x in min_x..=max_x {
            for y in min_y..=max_y { self.grid.entry((x, y)).or_default().push(entity); }
        }
    }
    pub fn query_aabb(&self, aabb: &Aabb) -> Vec<Entity> {
        let mut candidates = Vec::new();
        let min_x = (aabb.min[0] / self.cell_size).floor() as i32;
        let min_y = (aabb.min[1] / self.cell_size).floor() as i32;
        let max_x = (aabb.max[0] / self.cell_size).floor() as i32;
        let max_y = (aabb.max[1] / self.cell_size).floor() as i32;
        for x in min_x..=max_x {
            for y in min_y..=max_y {
                if let Some(entities) = self.grid.get(&(x, y)) { candidates.extend_from_slice(entities); }
            }
        }
        candidates
    }
}

pub fn broad_phase_system(world: &mut World) {
    let mut physics = PhysicsWorld::new(10.0);
    let query_state = world.query_state::<(Entity, &Aabb), ()>();
    for (entity, aabb) in query_state.iter(world) { physics.insert(entity, aabb); }
    world.insert_resource(physics);
}

pub struct SpatialQuery<'w, 's, Q: QueryMut, F: QueryFilter> {
    state: &'s mut QueryState<Q, F>,
    world: &'w mut World,
}

impl<'w, 's, Q: QueryMut, F: QueryFilter> SpatialQuery<'w, 's, Q, F> {
    pub fn new(state: &'s mut QueryState<Q, F>, world: &'w mut World) -> Self { Self { state, world } }
    pub fn iter_mut(&mut self, aabb: &Aabb) -> Vec<Q::Item<'_>> {
        let physics = self.world.get_resource::<PhysicsWorld>().expect("PhysicsWorld not found");
        let candidates = physics.query_aabb(aabb);
        let mut seen = std::collections::HashSet::new();
        let unique_candidates: Vec<Entity> = candidates.into_iter().filter(|e| seen.insert(*e)).collect();
        self.state.get_many_mut(self.world, &unique_candidates).into_iter().flatten().collect()
    }
}
