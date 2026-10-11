use crate::world::World;
use std::cell::UnsafeCell;

pub struct UnsafeWorldCell<'w>(*mut World, std::marker::PhantomData<&'w UnsafeCell<World>>);

impl<'w> UnsafeWorldCell<'w> {
    #[inline]
    pub(crate) fn new(world: &'w mut World) -> Self {
        Self(world as *mut World, std::marker::PhantomData)
    }
    #[inline] pub fn world(&self) -> &'w World { unsafe { &*self.0 } }
    #[inline] pub unsafe fn world_mut(&mut self) -> &'w mut World { &mut *self.0 }
}
