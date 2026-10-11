use crate::cell::UnsafeWorldCell;
use crate::commands::{CommandQueue, Commands};
use crate::query::{QueryFilter, QueryMut, QueryState};
use crate::resource::{Resource, Res, ResMut};
use std::any::TypeId;
use std::collections::HashSet;

#[derive(Default, Clone)]
pub struct Access {
    pub reads: HashSet<TypeId>,
    pub writes: HashSet<TypeId>,
    pub has_commands: bool,
    pub non_send: bool,
}

impl Access {
    pub fn is_compatible(&self, other: &Access) -> bool {
        let write_conflict = self.writes.intersection(&other.writes).next().is_some() ||
                             self.writes.intersection(&other.reads).next().is_some() ||
                             self.reads.intersection(&other.writes).next().is_some();
        let non_send_conflict = (self.non_send || other.non_send) && (self != other);
        !write_conflict && !non_send_conflict
    }
}

pub trait SystemParam {
    type Item<'w>;
    unsafe fn fetch<'w>(world: UnsafeWorldCell<'w>, queue: &'w mut CommandQueue) -> Self::Item<'w>;
    fn access() -> Access;
}

pub struct Query<'w, Q: QueryMut, F: QueryFilter> {
    pub state: QueryState<Q, F>,
    pub world: UnsafeWorldCell<'w>,
}

impl<'w, Q: QueryMut, F: QueryFilter> Query<'w, Q, F> {
    pub fn iter_mut(&mut self) -> crate::query::QueryMutIter<'_, Q, F> {
        unsafe { self.state.iter_mut(self.world.world_mut()) }
    }
}

impl<Q: QueryMut, F: QueryFilter> SystemParam for Query<'_, Q, F> {
    type Item<'w> = Query<'w, Q, F>;
    unsafe fn fetch<'w>(world: UnsafeWorldCell<'w>, _queue: &'w mut CommandQueue) -> Self::Item<'w> {
        Query { state: world.world().query_state::<Q, F>(), world }
    }
    fn access() -> Access { Access::default() }
}

impl<R: Resource> SystemParam for Res<'_, R> {
    type Item<'w> = Res<'w, R>;
    unsafe fn fetch<'w>(world: UnsafeWorldCell<'w>, _queue: &'w mut CommandQueue) -> Self::Item<'w> {
        world.world().get_resource::<R>().expect("Resource not found")
    }
    fn access() -> Access {
        let mut access = Access::default();
        access.reads.insert(TypeId::of::<R>());
        access
    }
}

impl<R: Resource> SystemParam for ResMut<'_, R> {
    type Item<'w> = ResMut<'w, R>;
    unsafe fn fetch<'w>(world: UnsafeWorldCell<'w>, _queue: &'w mut CommandQueue) -> Self::Item<'w> {
        world.world_mut().get_resource_mut::<R>().expect("Resource not found")
    }
    fn access() -> Access {
        let mut access = Access::default();
        access.writes.insert(TypeId::of::<R>());
        access
    }
}

impl SystemParam for Commands<'_> {
    type Item<'w> = Commands<'w>;
    unsafe fn fetch<'w>(world: UnsafeWorldCell<'w>, queue: &'w mut CommandQueue) -> Self::Item<'w> {
        Commands::new(world.world_mut(), queue)
    }
    fn access() -> Access {
        let mut access = Access::default();
        access.has_commands = true;
        access
    }
}

pub struct NonSendMut<'w, T: 'static> { pub(crate) value: &'w mut T }

impl<T: 'static> SystemParam for NonSendMut<'_, T> {
    type Item<'w> = NonSendMut<'w, T>;
    unsafe fn fetch<'w>(world: UnsafeWorldCell<'w>, _queue: &'w mut CommandQueue) -> Self::Item<'w> {
        let val = world.world_mut().get_non_send_resource_mut::<T>().expect("NonSend resource not found");
        NonSendMut { value: val }
    }
    fn access() -> Access {
        let mut access = Access::default();
        access.writes.insert(TypeId::of::<T>());
        access.non_send = true;
        access
    }
}

pub struct Local<'w, T: Resource + Default> { pub(crate) value: &'w mut T }

impl<T: Resource + Default> SystemParam for Local<'_, T> {
    type Item<'w> = Local<'w, T>;
    unsafe fn fetch<'w>(world: UnsafeWorldCell<'w>, _queue: &'w mut CommandQueue) -> Self::Item<'w> {
        let w = world.world_mut();
        if w.get_resource::<T>().is_none() { w.insert_resource(T::default()); }
        Local { value: w.get_resource_mut::<T>().unwrap().deref_mut() }
    }
    fn access() -> Access {
        let mut access = Access::default();
        access.writes.insert(TypeId::of::<T>());
        access
    }
}

impl<T> std::ops::Deref for Local<'_, T> {
    type Target = T;
    fn deref(&self) -> &Self::Target { self.value }
}
impl<T> std::ops::DerefMut for Local<'_, T> {
    fn deref_mut(&mut self) -> &mut Self::Target { self.value }
}

macro_rules! impl_system_param_tuple {
    ($($P: ident),*) => {
        impl<$($P: SystemParam),*> SystemParam for ($($P,)*) {
            type Item<'w> = ($($P::Item<'w>,)*);
            #[allow(unused_variables)]
            unsafe fn fetch<'w>(world: UnsafeWorldCell<'w>, queue: &'w mut CommandQueue) -> Self::Item<'w> {
                ($($P::fetch(world, queue),)*)
            }
            fn access() -> Access {
                let mut access = Access::default();
                $(
                    let p_access = $P::access();
                    access.reads.extend(p_access.reads);
                    access.writes.extend(p_access.writes);
                    if p_access.has_commands { access.has_commands = true; }
                    if p_access.non_send { access.non_send = true; }
                )*
                access
            }
        }
    }
}

impl_system_param_tuple!();
impl_system_param_tuple!(A);
impl_system_param_tuple!(A, B);
impl_system_param_tuple!(A, B, C);
impl_system_param_tuple!(A, B, C, D);

pub trait SystemParamFunction<Marker> {
    fn run(&mut self, world: UnsafeWorldCell<'_>, queue: &mut CommandQueue);
    fn access() -> Access;
}

macro_rules! impl_system_param_function {
    ($($P: ident),*) => {
        impl<F, $($P: SystemParam),*> SystemParamFunction<fn($($P),*)> for F
        where
            F: FnMut($($P),*) + FnMut($($P::Item<'_>),*) + Send + Sync + 'static,
        {
            fn run(&mut self, world: UnsafeWorldCell<'_>, queue: &mut CommandQueue) {
                let ($($P,)*) = unsafe { <$($P),*>::fetch(world, queue) };
                (self)($($P),*)
            }
            fn access() -> Access { <$($P),*>::access() }
        }
    }
}

impl_system_param_function!();
impl_system_param_function!(A);
impl_system_param_function!(A, B);
impl_system_param_function!(A, B, C);
impl_system_param_function!(A, B, C, D);
