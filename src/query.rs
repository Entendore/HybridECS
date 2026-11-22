use crate::archetype::Archetype;
use crate::component::{Component, ComponentPtr, Mut};
use crate::entity::{ArchetypeId, ComponentTypeId, Entity, Tick};
use crate::world::World;
use core::marker::PhantomData;
use core::ptr::NonNull;
use rayon::iter::plumbing::UnindexedConsumer;
use rayon::prelude::*;

pub trait Query: private::SealedQuery {
    type Item<'a>;
    type Fetch;
    type State;
    fn init_state(world: &World) -> Self::State;
    fn matches(state: &Self::State, archetype: &Archetype) -> bool;
    unsafe fn init_fetch(archetype: &Archetype, state: &Self::State) -> Option<Self::Fetch>;
    unsafe fn fetch<'a>(fetch: &mut Self::Fetch, entity: Entity, index: usize, current_tick: Tick, last_change_tick: Tick) -> Self::Item<'a>;
}

pub trait QueryMut: private::SealedQueryMut {
    type Item<'a>;
    type Fetch;
    type State;
    fn init_state(world: &World) -> Self::State;
    fn matches(state: &Self::State, archetype: &Archetype) -> bool;
    unsafe fn init_fetch_mut(archetype: &mut Archetype, state: &Self::State) -> Option<Self::Fetch>;
    unsafe fn fetch<'a>(fetch: &Self::Fetch, entity: Entity, index: usize, current_tick: Tick, last_change_tick: Tick) -> Self::Item<'a>;
}

pub trait QueryFilter: private::SealedFilter {
    type State;
    fn init_state(world: &World) -> Self::State;
    fn matches_archetype(state: &Self::State, archetype: &Archetype) -> bool;
    #[inline]
    unsafe fn matches_entity(state: &Self::State, archetype: &Archetype, index: usize, last_change_tick: Tick) -> bool {
        let _ = (state, archetype, index, last_change_tick);
        true
    }
}

pub trait ReadOnlyQuery: Query {}

impl<T: Component> ReadOnlyQuery for &T {}
impl<T: Component> ReadOnlyQuery for Option<&T> {}
impl ReadOnlyQuery for () {}

macro_rules! impl_read_only_query_tuple {
    ($($T:ident),+) => { impl<$($T: ReadOnlyQuery),+> ReadOnlyQuery for ($($T,)+) {} }
}
impl_read_only_query_tuple!(A);
impl_read_only_query_tuple!(A, B);
impl_read_only_query_tuple!(A, B, C);
impl_read_only_query_tuple!(A, B, C, D);

impl<T: Component> private::SealedQuery for &T {}
impl<T: Component> private::SealedQuery for Option<&T> {}
impl private::SealedQuery for () {}

macro_rules! impl_sealed_query_tuple {
    ($($T:ident),+) => { impl<$($T: private::SealedQuery),+> private::SealedQuery for ($($T,)+) {} }
}
impl_sealed_query_tuple!(A);
impl_sealed_query_tuple!(A, B);
impl_sealed_query_tuple!(A, B, C);
impl_sealed_query_tuple!(A, B, C, D);

impl<T: Component> private::SealedQueryMut for &T {}
impl<T: Component> private::SealedQueryMut for &mut T {}
impl<T: Component> private::SealedQueryMut for Option<&T> {}
impl<T: Component> private::SealedQueryMut for Option<&mut T> {}
impl private::SealedQueryMut for () {}

macro_rules! impl_sealed_query_mut_tuple {
    ($($T:ident),+) => { impl<$($T: private::SealedQueryMut),+> private::SealedQueryMut for ($($T,)+) {} }
}
impl_sealed_query_mut_tuple!(A);
impl_sealed_query_mut_tuple!(A, B);
impl_sealed_query_mut_tuple!(A, B, C);
impl_sealed_query_mut_tuple!(A, B, C, D);

impl<T: Component> private::SealedFilter for With<T> {}
impl<T: Component> private::SealedFilter for Without<T> {}
impl<T: Component> private::SealedFilter for Added<T> {}
impl<T: Component> private::SealedFilter for Changed<T> {}
impl private::SealedFilter for () {}

macro_rules! impl_sealed_filter_tuple {
    ($($T:ident),+) => { impl<$($T: private::SealedFilter),+> private::SealedFilter for ($($T,)+) {} }
}
impl_sealed_filter_tuple!(A);
impl_sealed_filter_tuple!(A, B);
impl_sealed_filter_tuple!(A, B, C);
impl_sealed_filter_tuple!(A, B, C, D);

pub struct Or<T>(PhantomData<T>);

pub trait OrFilter {
    type State;
    fn init_state(world: &World) -> Self::State;
    fn matches_archetype(state: &Self::State, archetype: &Archetype) -> bool;
    unsafe fn matches_entity(state: &Self::State, archetype: &Archetype, index: usize, last_change_tick: Tick) -> bool {
        let _ = (state, archetype, index, last_change_tick);
        true
    }
}

impl OrFilter for () {
    type State = ();
    fn init_state(_world: &World) -> Self::State {}
    fn matches_archetype(_state: &Self::State, _archetype: &Archetype) -> bool { false }
}

macro_rules! impl_or_filter {
    ($($T:ident),+; $($i:tt),+) => {
        impl<$($T: QueryFilter),+> OrFilter for ($($T,)+) {
            type State = ($($T::State,)+);
            fn init_state(world: &World) -> Self::State { ($($T::init_state(world),)+) }
            fn matches_archetype(state: &Self::State, archetype: &Archetype) -> bool {
                false $( || $T::matches_archetype(&state.$i, archetype) )+
            }
        }
    }
}
impl_or_filter!(A; 0);
impl_or_filter!(A, B; 0, 1);
impl_or_filter!(A, B, C; 0, 1, 2);

impl<T: OrFilter> QueryFilter for Or<T> {
    type State = T::State;
    fn init_state(world: &World) -> Self::State { T::init_state(world) }
    fn matches_archetype(state: &Self::State, archetype: &Archetype) -> bool { T::matches_archetype(state, archetype) }
}

pub struct QueryState<Q, F = ()> {
    pub(crate) state: Q::State,
    pub(crate) filter_state: F::State,
    pub(crate) matched_archetypes: Vec<ArchetypeId>,
    pub(crate) last_archetype_count: usize,
    pub(crate) archetype_generation: usize,
    pub(crate) matched_entity_count: usize,
}

impl<Q: Query, F: QueryFilter> QueryState<Q, F> {
    pub fn new(world: &World) -> Self {
        Self {
            state: Q::init_state(world),
            filter_state: F::init_state(world),
            matched_archetypes: Vec::new(),
            last_archetype_count: 0,
            archetype_generation: 0,
            matched_entity_count: 0,
        }
    }

    pub fn update_archetypes(&mut self, world: &World) {
        if world.archetype_generation == self.archetype_generation { return; }
        let start_idx = if world.archetype_generation > self.archetype_generation { self.last_archetype_count } else { self.matched_archetypes.clear(); self.matched_entity_count = 0; 0 };
        for i in start_idx..world.archetypes.len() {
            let arch = &world.archetypes[i];
            if Q::matches(&self.state, arch) && F::matches_archetype(&self.filter_state, arch) {
                self.matched_entity_count += arch.len();
                self.matched_archetypes.push(arch.id);
            }
        }
        self.last_archetype_count = world.archetypes.len();
        self.archetype_generation = world.archetype_generation;
    }
    
    pub fn is_empty(&self, world: &World) -> bool {
        if world.archetype_generation != self.archetype_generation {
            return world.archetypes.iter().all(|arch| arch.is_empty() || !Q::matches(&self.state, arch) || !F::matches_archetype(&self.filter_state, arch));
        }
        self.matched_entity_count == 0
    }
}

impl<Q: QueryMut, F: QueryFilter> QueryState<Q, F> {
    pub fn iter_mut<'w>(&'w mut self, world: &'w mut World) -> QueryMutIter<'w, Q, F> {
        self.update_archetypes(world);
        QueryMutIter::new(world, self)
    }
    pub fn get_mut<'w>(&mut self, world: &'w mut World, entity: Entity) -> Option<Q::Item<'w>> {
        self.update_archetypes(world);
        let loc = world.entity_location(entity)?;
        let arch = &mut world.archetypes[loc.archetype().get() as usize - 1];
        if !Q::matches(&self.state, arch) || !F::matches_archetype(&self.filter_state, arch) { return None; }
        let lct = world.last_change_tick;
        let ct = world.current_tick;
        unsafe {
            let mut fetch = Q::init_fetch_mut(arch, &self.state)?;
            if !F::matches_entity(&self.filter_state, arch, loc.index() as usize, lct) { return None; }
            Some(Q::fetch(&fetch, entity, loc.index() as usize, ct, lct))
        }
    }
}

impl<Q: ReadOnlyQuery, F: QueryFilter> QueryState<Q, F> {
    pub fn par_iter<'w>(&self, world: &'w World) -> QueryParIter<'w, Q, F> {
        QueryParIter {
            world,
            state: &self.state,
            filter_state: &self.filter_state,
            current_tick: world.current_tick,
            last_change_tick: world.last_change_tick,
        }
    }
}

pub struct With<T>(PhantomData<T>);
pub struct Without<T>(PhantomData<T>);
pub struct Added<T>(PhantomData<T>);
pub struct Changed<T>(PhantomData<T>);

impl<T: Component> QueryFilter for With<T> {
    type State = Option<ComponentTypeId>;
    fn init_state(world: &World) -> Self::State { world.component_id::<T>() }
    fn matches_archetype(state: &Self::State, archetype: &Archetype) -> bool {
        state.map(|cid| archetype.signature.contains(cid.get())).unwrap_or(false)
    }
}

impl<T: Component> QueryFilter for Without<T> {
    type State = Option<ComponentTypeId>;
    fn init_state(world: &World) -> Self::State { world.component_id::<T>() }
    fn matches_archetype(state: &Self::State, archetype: &Archetype) -> bool {
        state.map(|cid| !archetype.signature.contains(cid.get())).unwrap_or(true)
    }
}

impl<T: Component> QueryFilter for Added<T> {
    type State = Option<ComponentTypeId>;
    fn init_state(world: &World) -> Self::State { world.component_id::<T>() }
    fn matches_archetype(state: &Self::State, archetype: &Archetype) -> bool {
        if let Some(cid) = state {
            if let Some(col) = archetype.column(*cid) { return !col.added_bits().0.is_empty(); }
        }
        false
    }
    #[inline] unsafe fn matches_entity(state: &Self::State, archetype: &Archetype, index: usize, _last_change_tick: Tick) -> bool {
        if let Some(cid) = state {
            if let Some(col) = archetype.column(*cid) { return col.added_bits().contains(index as u32); }
        }
        false
    }
}

impl<T: Component> QueryFilter for Changed<T> {
    type State = Option<ComponentTypeId>;
    fn init_state(world: &World) -> Self::State { world.component_id::<T>() }
    fn matches_archetype(state: &Self::State, archetype: &Archetype) -> bool {
        if let Some(cid) = state {
            if let Some(col) = archetype.column(*cid) { return !col.changed_bits().0.is_empty(); }
        }
        false
    }
    #[inline] unsafe fn matches_entity(state: &Self::State, archetype: &Archetype, index: usize, _last_change_tick: Tick) -> bool {
        if let Some(cid) = state {
            if let Some(col) = archetype.column(*cid) { return col.changed_bits().contains(index as u32); }
        }
        false
    }
}

impl QueryFilter for () {
    type State = ();
    fn init_state(_world: &World) -> Self::State {}
    fn matches_archetype(_state: &Self::State, _archetype: &Archetype) -> bool { true }
}

macro_rules! impl_filter_tuple {
    ($($T:ident),+; $($i:tt),+) => {
        impl<$($T: QueryFilter),+> QueryFilter for ($($T,)+) {
            type State = ($($T::State,)+);
            fn init_state(world: &World) -> Self::State { ($($T::init_state(world),)+) }
            fn matches_archetype(state: &Self::State, archetype: &Archetype) -> bool {
                true $( && $T::matches_archetype(&state.$i, archetype) )+
            }
            #[inline] unsafe fn matches_entity(state: &Self::State, archetype: &Archetype, index: usize, last_change_tick: Tick) -> bool {
                true $( && $T::matches_entity(&state.$i, archetype, index, last_change_tick) )+
            }
        }
    }
}
impl_filter_tuple!(A; 0);
impl_filter_tuple!(A, B; 0, 1);
impl_filter_tuple!(A, B, C; 0, 1, 2);
impl_filter_tuple!(A, B, C, D; 0, 1, 2, 3);

pub struct EntityRef;
impl Query for EntityRef {
    type Item<'a> = Entity;
    type Fetch = NonNull<Entity>;
    type State = ();
    fn init_state(_world: &World) -> Self::State {}
    fn matches(_state: &Self::State, _archetype: &Archetype) -> bool { true }
    unsafe fn init_fetch(archetype: &Archetype, _state: &Self::State) -> Option<Self::Fetch> {
        NonNull::new(archetype.entities.as_ptr() as *mut Entity)
    }
    unsafe fn fetch<'a>(fetch: &mut Self::Fetch, _entity: Entity, index: usize, _ct: Tick, _lct: Tick) -> Self::Item<'a> {
        *fetch.as_ptr().add(index)
    }
}
impl QueryMut for EntityRef {
    type Item<'a> = Entity;
    type Fetch = NonNull<Entity>;
    type State = ();
    fn init_state(_world: &World) -> Self::State {}
    fn matches(_state: &Self::State, _archetype: &Archetype) -> bool { true }
    unsafe fn init_fetch_mut(archetype: &mut Archetype, _state: &Self::State) -> Option<Self::Fetch> {
        NonNull::new(archetype.entities.as_ptr() as *mut Entity)
    }
    unsafe fn fetch<'a>(fetch: &Self::Fetch, _entity: Entity, index: usize, _ct: Tick, _lct: Tick) -> Self::Item<'a> {
        *fetch.as_ptr().add(index)
    }
}

impl<T: Component> Query for &T {
    type Item<'a> = &'a T;
    type Fetch = ComponentPtr<T>;
    type State = Option<ComponentTypeId>;
    fn init_state(world: &World) -> Self::State { world.component_id::<T>() }
    fn matches(state: &Self::State, arch: &Archetype) -> bool { state.map(|cid| arch.signature.contains(cid.get())).unwrap_or(false) }
    unsafe fn init_fetch(archetype: &Archetype, state: &Self::State) -> Option<Self::Fetch> {
        let cid = (*state)?;
        archetype.column(cid).map(|col| { let ptr = col.get_ptr(0); ComponentPtr::new(ptr.cast::<T>(), col.get_tick_ptr(0)) })
    }
    unsafe fn fetch<'a>(fetch: &mut Self::Fetch, _entity: Entity, index: usize, _ct: Tick, _lct: Tick) -> Self::Item<'a> {
        fetch.get(index).0
    }
}

impl<T: Component> QueryMut for &T {
    type Item<'a> = &'a T;
    type Fetch = ComponentPtr<T>;
    type State = Option<ComponentTypeId>;
    fn init_state(world: &World) -> Self::State { world.component_id::<T>() }
    fn matches(state: &Self::State, arch: &Archetype) -> bool { state.map(|cid| arch.signature.contains(cid.get())).unwrap_or(false) }
    unsafe fn init_fetch_mut(archetype: &mut Archetype, state: &Self::State) -> Option<Self::Fetch> {
        let cid = (*state)?;
        archetype.column(cid).map(|col| { let ptr = col.get_ptr(0); ComponentPtr::new(ptr.cast::<T>(), col.get_tick_ptr(0)) })
    }
    unsafe fn fetch<'a>(fetch: &Self::Fetch, _entity: Entity, index: usize, _ct: Tick, _lct: Tick) -> Self::Item<'a> {
        fetch.get(index).0
    }
}

impl<T: Component> QueryMut for &mut T {
    type Item<'a> = Mut<'a, T>;
    type Fetch = ComponentPtr<T>;
    type State = Option<ComponentTypeId>;
    fn init_state(world: &World) -> Self::State { world.component_id::<T>() }
    fn matches(state: &Self::State, arch: &Archetype) -> bool { state.map(|cid| arch.signature.contains(cid.get())).unwrap_or(false) }
    unsafe fn init_fetch_mut(archetype: &mut Archetype, state: &Self::State) -> Option<Self::Fetch> {
        let cid = (*state)?;
        archetype.column_mut(cid).map(|col| { let ptr = col.get_ptr(0); ComponentPtr::new(ptr.cast::<T>(), col.get_tick_ptr(0)) })
    }
    unsafe fn fetch<'a>(fetch: &Self::Fetch, _entity: Entity, index: usize, ct: Tick, lct: Tick) -> Self::Item<'a> {
        let (value, tick) = fetch.get(index);
        Mut { value, tick: tick as *const u32 as *mut u32, changed_bits: None, index: index as u32, current_tick: ct, last_change_tick: lct }
    }
}

macro_rules! impl_query_tuple {
    ($($T:ident),+; $($i:tt),+) => {
        impl<$($T: Query),+> Query for ($($T,)+) {
            type Item<'a> = ($($T::Item<'a>,)+);
            type Fetch = ($($T::Fetch,)+);
            type State = ($($T::State,)+);
            fn init_state(world: &World) -> Self::State { ($($T::init_state(world),)+) }
            fn matches(state: &Self::State, archetype: &Archetype) -> bool { true $( && $T::matches(&state.$i, archetype) )+ }
            unsafe fn init_fetch(archetype: &Archetype, state: &Self::State) -> Option<Self::Fetch> { Some(($($T::init_fetch(archetype, &state.$i)?,)+)) }
            unsafe fn fetch<'a>(fetch: &mut Self::Fetch, entity: Entity, index: usize, ct: Tick, lct: Tick) -> Self::Item<'a> { ($($T::fetch(&mut fetch.$i, entity, index, ct, lct),)+) }
        }
        impl<$($T: QueryMut),+> QueryMut for ($($T,)+) {
            type Item<'a> = ($($T::Item<'a>,)+);
            type Fetch = ($($T::Fetch,)+);
            type State = ($($T::State,)+);
            fn init_state(world: &World) -> Self::State { ($($T::init_state(world),)+) }
            fn matches(state: &Self::State, archetype: &Archetype) -> bool { true $( && $T::matches(&state.$i, archetype) )+ }
            unsafe fn init_fetch_mut(archetype: &mut Archetype, state: &Self::State) -> Option<Self::Fetch> {
                let arch_ptr = archetype as *mut Archetype;
                Some(($({ let arch = &mut *arch_ptr; $T::init_fetch_mut(arch, &state.$i)?},)+))
            }
            unsafe fn fetch<'a>(fetch: &Self::Fetch, entity: Entity, index: usize, ct: Tick, lct: Tick) -> Self::Item<'a> { ($($T::fetch(&fetch.$i, entity, index, ct, lct),)+) }
        }
    }
}
impl_query_tuple!(A; 0);
impl_query_tuple!(A, B; 0, 1);
impl_query_tuple!(A, B, C; 0, 1, 2);
impl_query_tuple!(A, B, C, D; 0, 1, 2, 3);

#[must_use]
pub struct QueryIter<'w, Q: Query, F: QueryFilter> {
    world: &'w World,
    state: &'w Q::State,
    filter_state: &'w F::State,
    matched_idx: usize,
    entity_idx: usize,
    current: Option<(Q::Fetch, usize, usize)>,
    len: usize,
    current_tick: Tick,
    last_change_tick: Tick,
}

impl<'w, Q: Query, F: QueryFilter> Iterator for QueryIter<'w, Q, F> {
    type Item = Q::Item<'w>;
    #[inline]
    fn next(&mut self) -> Option<Self::Item<'w>> {
        loop {
            if let Some((fetch, len, arch_idx)) = self.current.as_mut() {
                if self.entity_idx < *len {
                    let idx = self.entity_idx;
                    self.entity_idx += 1;
                    let arch = &self.world.archetypes[*arch_idx];
                    let entity = arch.entities[idx];
                    if unsafe { F::matches_entity(self.filter_state, arch, idx, self.last_change_tick) } {
                        return Some(unsafe { Q::fetch(fetch, entity, idx, self.current_tick, self.last_change_tick) });
                    }
                    continue;
                }
            }
            self.current = None;
            while self.matched_idx < self.world.archetypes.len() {
                let i = self.matched_idx;
                self.matched_idx += 1;
                let arch = &self.world.archetypes[i];
                if arch.is_empty() { continue; }
                if !Q::matches(self.state, arch) { continue; }
                if !F::matches_archetype(self.filter_state, arch) { continue; }
                if let Some(fetch) = unsafe { Q::init_fetch(arch, self.state) } {
                    self.entity_idx = 0;
                    let len = arch.len();
                    self.current = Some((fetch, len, i));
                    break;
                }
            }
            if self.current.is_none() { return None; }
        }
    }
    #[inline] fn size_hint(&self) -> (usize, Option<usize>) { (self.len, Some(self.len)) }
}
impl<'w, Q: Query, F: QueryFilter> ExactSizeIterator for QueryIter<'w, Q, F> {}
unsafe impl<'w, Q: Query, F: QueryFilter> core::iter::TrustedLen for QueryIter<'w, Q, F> {}
impl<'w, Q: Query, F: QueryFilter> core::iter::FusedIterator for QueryIter<'w, Q, F> {}

#[must_use]
pub struct QueryMutIter<'w, Q: QueryMut, F: QueryFilter> {
    world: &'w mut World,
    state: &'w Q::State,
    filter_state: &'w F::State,
    matched_idx: usize,
    entity_idx: usize,
    current: Option<(Q::Fetch, usize, usize)>,
    len: usize,
    current_tick: Tick,
    last_change_tick: Tick,
}

impl<'w, Q: QueryMut, F: QueryFilter> Iterator for QueryMutIter<'w, Q, F> {
    type Item = Q::Item<'w>;
    #[inline]
    fn next(&mut self) -> Option<Self::Item<'w>> {
        loop {
            if let Some((fetch, len, arch_idx)) = self.current.as_mut() {
                if self.entity_idx < *len {
                    let idx = self.entity_idx;
                    self.entity_idx += 1;
                    let arch = &self.world.archetypes[*arch_idx];
                    let entity = arch.entities[idx];
                    if unsafe { F::matches_entity(self.filter_state, arch, idx, self.last_change_tick) } {
                        return Some(unsafe { Q::fetch(fetch, entity, idx, self.current_tick, self.last_change_tick) });
                    }
                    continue;
                }
            }
            self.current = None;
            while self.matched_idx < self.world.archetypes.len() {
                let i = self.matched_idx;
                self.matched_idx += 1;
                let arch = &mut self.world.archetypes[i];
                if arch.is_empty() { continue; }
                if !Q::matches(self.state, arch) { continue; }
                if !F::matches_archetype(self.filter_state, arch) { continue; }
                if let Some(fetch) = unsafe { Q::init_fetch_mut(arch, self.state) } {
                    self.entity_idx = 0;
                    let len = arch.len();
                    self.current = Some((fetch, len, i));
                    break;
                }
            }
            if self.current.is_none() { return None; }
        }
    }
    #[inline] fn size_hint(&self) -> (usize, Option<usize>) { (self.len, Some(self.len)) }
}
impl<'w, Q: QueryMut, F: QueryFilter> ExactSizeIterator for QueryMutIter<'w, Q, F> {}
unsafe impl<'w, Q: QueryMut, F: QueryFilter> core::iter::TrustedLen for QueryMutIter<'w, Q, F> {}
impl<'w, Q: QueryMut, F: QueryFilter> core::iter::FusedIterator for QueryMutIter<'w, Q, F> {}

#[must_use]
pub struct QueryParIter<'w, Q: ReadOnlyQuery, F: QueryFilter> {
    world: &'w World,
    state: &'w Q::State,
    filter_state: &'w F::State,
    current_tick: Tick,
    last_change_tick: Tick,
}

impl<'w, Q: ReadOnlyQuery, F: QueryFilter> QueryParIter<'w, Q, F>
where Q::Fetch: Send, Q::State: Sync, F::State: Sync, Q::Item<'_>: Send {
    pub fn for_each<Func>(self, func: Func)
    where Func: Fn(Q::Item<'_>) + Send + Sync {
        self.into_par_iter().for_each(func);
    }
}

impl<'w, Q: ReadOnlyQuery, F: QueryFilter> ParallelIterator for QueryParIter<'w, Q, F>
where Q::Fetch: Send, Q::State: Sync, F::State: Sync, Q::Item<'_>: Send {
    type Item = Q::Item<'w>;
    fn drive_unindexed<C>(self, consumer: C) -> C::Result
    where C: UnindexedConsumer<Self::Item> {
        let state = self.state;
        let filter_state = self.filter_state;
        let ct = self.current_tick;
        let lct = self.last_change_tick;
        self.world.archetypes.par_iter().flat_map_iter(|arch| {
            if arch.is_empty() || !Q::matches(state, arch) || !F::matches_archetype(filter_state, arch) {
                Vec::new().into_iter()
            } else {
                unsafe {
                    Q::init_fetch(arch, state).map(|fetch| {
                        let len = arch.len();
                        (0..len).filter(move |&i| F::matches_entity(filter_state, arch, i, lct))
                            .map(move |i| Q::fetch(&fetch, arch.entities[i], i, ct, lct)).collect::<Vec<_>>().into_iter()
                    }).unwrap_or_default()
                }
            }
        }).drive_unindexed(consumer)
    }
}
