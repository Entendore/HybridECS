use crate::component::Component;
use crate::entity::Entity;
use crate::world::World;
use std::any::{Any, TypeId};
use std::collections::HashMap;

pub trait Reflect: Any + Send + Sync {
    fn type_name(&self) -> &'static str;
    fn as_any(&self) -> &dyn Any;
    fn clone_value(&self) -> Box<dyn Reflect>;
}

pub trait FromReflect: Reflect {
    fn from_reflect(reflect: &dyn Reflect) -> Option<Self> where Self: Sized;
}

#[derive(Default)]
pub struct TypeRegistry {
    pub(crate) component_fns: HashMap<TypeId, ReflectComponentFns>,
}

pub(crate) struct ReflectComponentFns {
    pub name: &'static str,
    pub insert: fn(&mut World, Entity, Box<dyn Reflect>),
}

impl TypeRegistry {
    pub fn register<T: Reflect + Component + Clone>(&mut self) {
        fn insert<T: Reflect + Component + Clone>(world: &mut World, entity: Entity, data: Box<dyn Reflect>) {
            if let Some(val) = data.as_any().downcast_ref::<T>() { world.insert(entity, val.clone()); }
        }
        self.component_fns.insert(TypeId::of::<T>(), ReflectComponentFns { name: std::any::type_name::<T>(), insert: insert::<T> });
    }
}

impl World {
    pub fn insert_reflected(&mut self, entity: Entity, type_id: TypeId, data: Box<dyn Reflect>) {
        let registry = self.get_resource::<TypeRegistry>().expect("TypeRegistry not found");
        if let Some(fns) = registry.component_fns.get(&type_id) { (fns.insert)(self, entity, data); }
        else { panic!("Component {:?} not registered in TypeRegistry", type_id); }
    }
}
