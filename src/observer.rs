use crate::component::Component;
use crate::entity::Entity;
use crate::world::World;
use std::any::TypeId;

#[derive(Component)]
pub struct Observer<E: 'static> {
    pub(crate) event_type: TypeId,
    pub(crate) callback: Box<dyn FnMut(&mut World, Entity, &E) + Send + Sync>,
}

impl<E: 'static> Observer<E> {
    pub fn new(callback: impl FnMut(&mut World, Entity, &E) + Send + Sync + 'static) -> Self {
        Self { event_type: TypeId::of::<E>(), callback: Box::new(callback) }
    }
}

impl World {
    pub fn trigger<E: 'static>(&mut self, entity: Entity, event: E) {
        let has_observer = self.get::<Observer<E>>(entity).map(|o| o.event_type == TypeId::of::<E>()).unwrap_or(false);
        if has_observer {
            let mut cb = std::mem::replace(&mut self.get_mut::<Observer<E>>(entity).unwrap().callback, Box::new(|_, _, _| {}));
            cb(self, entity, &event);
            if let Some(mut obs) = self.get_mut::<Observer<E>>(entity) { obs.callback = cb; }
        }
    }
}
