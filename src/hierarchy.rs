use crate::component::Component;
use crate::entity::Entity;
use crate::network::MapEntities;

#[derive(Component, Debug, Clone, Copy)]
pub struct Parent(pub(crate) Entity);

impl Parent {
    #[inline] pub fn get(&self) -> Entity { self.0 }
}

impl MapEntities for Parent {
    fn map_entities(&mut self, entity_map: &crate::network::EntityMap) {
        self.0.map_entities(entity_map);
    }
}

#[derive(Component, Debug, Clone, Default)]
pub struct Children(pub(crate) Vec<Entity>);

impl Children {
    #[inline] pub fn iter(&self) -> impl Iterator<Item = Entity> + '_ { self.0.iter().copied() }
    #[inline] pub fn len(&self) -> usize { self.0.len() }
    #[inline] pub fn is_empty(&self) -> bool { self.0.is_empty() }
}

impl MapEntities for Children {
    fn map_entities(&mut self, entity_map: &crate::network::EntityMap) {
        self.0.map_entities(entity_map);
    }
}
