use crate::entity::Entity;
use crate::resource::Resource;
use std::collections::HashMap;

#[derive(Resource, Default)]
pub struct EntityMap {
    pub map: HashMap<Entity, Entity>,
}

impl EntityMap {
    pub fn insert(&mut self, source: Entity, destination: Entity) { self.map.insert(source, destination); }
    pub fn get(&self, source: Entity) -> Option<Entity> { self.map.get(&source).copied() }
}

pub trait MapEntities {
    fn map_entities(&mut self, entity_map: &EntityMap);
}

impl MapEntities for Entity {
    fn map_entities(&mut self, entity_map: &EntityMap) {
        if let Some(mapped) = entity_map.get(*self) { *self = mapped; }
    }
}

impl<T: MapEntities> MapEntities for Option<T> {
    fn map_entities(&mut self, entity_map: &EntityMap) {
        if let Some(inner) = self { inner.map_entities(entity_map); }
    }
}

impl<T: MapEntities> MapEntities for Vec<T> {
    fn map_entities(&mut self, entity_map: &EntityMap) {
        for item in self.iter_mut() { item.map_entities(entity_map); }
    }
}
