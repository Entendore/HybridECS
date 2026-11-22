use crate::archetype::Archetype;
use crate::component::Component;
use crate::entity::ComponentTypeId;
use crate::world::World;

pub trait Bundle: Send + Sync + 'static {
    fn component_ids(world: &World, ids: &mut Vec<ComponentTypeId>);
    unsafe fn put(self, archetype: &mut Archetype, ids: &[ComponentTypeId], tick: u32);
}

impl Bundle for () {
    fn component_ids(_world: &World, _ids: &mut Vec<ComponentTypeId>) {}
    unsafe fn put(self, _archetype: &mut Archetype, _ids: &[ComponentTypeId], _tick: u32) {}
}

macro_rules! impl_bundle_tuple {
    ($($T:ident),+; $($i:tt),+) => {
        impl<$($T: Component),+> Bundle for ($($T,)+) {
            fn component_ids(world: &World, ids: &mut Vec<ComponentTypeId>) {
                $(
                    ids.push(world.component_id::<$T>().unwrap_or_else(|| panic!("Component {} not registered", std::any::type_name::<$T>())));
                )+
            }
            unsafe fn put(self, archetype: &mut Archetype, ids: &[ComponentTypeId], tick: u32) {
                let this = std::mem::ManuallyDrop::new(self);
                $(
                    let bytes = &this.$i as *const $T as *const u8;
                    let cid = ids[$i];
                    if let Some(col) = archetype.column_mut(cid) {
                        col.push_raw(bytes, tick);
                    }
                )+
            }
        }
    }
}

impl_bundle_tuple!(A; 0);
impl_bundle_tuple!(A, B; 0, 1);
impl_bundle_tuple!(A, B, C; 0, 1, 2);
impl_bundle_tuple!(A, B, C, D; 0, 1, 2, 3);
impl_bundle_tuple!(A, B, C, D, E; 0, 1, 2, 3, 4);
impl_bundle_tuple!(A, B, C, D, E, F; 0, 1, 2, 3, 4, 5);
impl_bundle_tuple!(A, B, C, D, E, F, G; 0, 1, 2, 3, 4, 5, 6);
impl_bundle_tuple!(A, B, C, D, E, F, G, H; 0, 1, 2, 3, 4, 5, 6, 7);
