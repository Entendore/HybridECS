use crate::bitset::BitSet;
use crate::entity::{ComponentTypeId, Entity, Tick};
use crate::world::World;
use core::any::{Any, TypeId};
use core::ops::{Deref, DerefMut};
use core::ptr::NonNull;

pub trait Component: Any + Send + Sync {}
impl<T: Any + Send + Sync> Component for T {}

pub const CHUNK_SIZE: usize = 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StorageType {
    Archetype,
    SparseSet,
    Immutable,
}

pub struct Mut<'a, T: ?Sized + 'a> {
    pub(crate) value: &'a mut T,
    pub(crate) tick: &'a mut u32,
    pub(crate) changed_bits: Option<&'a mut BitSet>,
    pub(crate) index: u32,
    pub(crate) current_tick: Tick,
    pub(crate) last_change_tick: Tick,
}

impl<'a, T: ?Sized> Deref for Mut<'a, T> {
    type Target = T;
    #[inline] fn deref(&self) -> &Self::Target { self.value }
}

impl<'a, T: ?Sized> DerefMut for Mut<'a, T> {
    #[inline] fn deref_mut(&mut self) -> &mut Self::Target {
        *self.tick = self.current_tick.0;
        if let Some(bits) = self.changed_bits.as_mut() { bits.set(self.index); }
        self.value
    }
}

impl<'a, T: ?Sized> Mut<'a, T> {
    #[inline] pub fn is_changed(&self) -> bool { *self.tick > self.last_change_tick.0 }
    #[inline] pub fn last_changed(&self) -> Tick { Tick(*self.tick) }
    
    pub fn map<U: ?Sized, F: FnOnce(&mut T) -> &mut U>(mut orig: Mut<'a, T>, f: F) -> Mut<'a, U> {
        let value = f(orig.value);
        Mut {
            value,
            tick: orig.tick,
            changed_bits: orig.changed_bits.take(),
            index: orig.index,
            current_tick: orig.current_tick,
            last_change_tick: orig.last_change_tick,
        }
    }

    #[inline]
    pub fn reborrow(&mut self) -> Mut<'_, T> {
        Mut {
            value: self.value,
            tick: self.tick,
            changed_bits: self.changed_bits.as_mut().map(|b| &mut **b),
            index: self.index,
            current_tick: self.current_tick,
            last_change_tick: self.last_change_tick,
        }
    }
}

impl<T: ?Sized> AsRef<T> for Mut<'_, T> { #[inline] fn as_ref(&self) -> &T { self.value } }
impl<T: ?Sized> AsMut<T> for Mut<'_, T> { #[inline] fn as_mut(&mut self) -> &mut T { self.deref_mut() } }

#[derive(Clone, Copy)]
pub struct ComponentPtr<T> {
    ptr: NonNull<T>,
    tick: NonNull<u32>,
}

unsafe impl<T: Send> Send for ComponentPtr<T> {}
unsafe impl<T: Sync> Sync for ComponentPtr<T> {}

impl<T> ComponentPtr<T> {
    #[inline] pub(crate) unsafe fn new(ptr: NonNull<T>, tick: NonNull<u32>) -> Self { Self { ptr, tick } }
    #[inline] pub(crate) unsafe fn get(&self, index: usize) -> (&T, &u32) {
        (&*self.ptr.as_ptr().add(index), &*self.tick.as_ptr().add(index))
    }
}

pub(crate) struct ComponentInfo {
    pub(crate) type_id: TypeId,
    pub(crate) storage: StorageType,
    pub(crate) make_column: fn(ComponentTypeId) -> Box<dyn ComponentColumn>,
    pub(crate) on_add: Option<Box<dyn Fn(&mut World, Entity) + Send + Sync>>,
    pub(crate) on_insert: Option<Box<dyn Fn(&mut World, Entity) + Send + Sync>>,
    pub(crate) on_replace: Option<Box<dyn Fn(&mut World, Entity) + Send + Sync>>,
    pub(crate) on_remove: Option<Box<dyn Fn(&mut World, Entity) + Send + Sync>>,
}

pub(crate) trait ComponentColumn: Any + Send + Sync {
    fn as_any(&self) -> &dyn Any;
    fn as_any_mut(&mut self) -> &mut dyn Any;
    fn component_type_id(&self) -> ComponentTypeId;
    fn len(&self) -> usize;
    unsafe fn get_ptr(&self, index: usize) -> NonNull<u8>;
    unsafe fn get_tick_ptr(&self, index: usize) -> NonNull<u32>;
    fn added_bits(&self) -> &BitSet;
    fn changed_bits(&self) -> &BitSet;
    fn clear_change_bits(&mut self);
    fn reserve(&mut self, additional: usize);
    
    unsafe fn push_raw(&mut self, bytes: *const u8, tick: u32);
    unsafe fn transfer_out(&mut self, index: usize, dst: &mut dyn ComponentColumn, tick: u32);
    unsafe fn swap_remove_drop(&mut self, index: usize);
    unsafe fn swap_remove_read(&mut self, index: usize);
    unsafe fn clear(&mut self);
}

pub(crate) struct TypedColumn<T> {
    chunks: Vec<Vec<T>>,
    added_ticks: Vec<Vec<u32>>,
    mutated_ticks: Vec<Vec<u32>>,
    added_bits: BitSet,
    changed_bits: BitSet,
    component_type_id: ComponentTypeId,
}

impl<T: Component> TypedColumn<T> {
    fn new(id: ComponentTypeId) -> Self {
        Self {
            chunks: Vec::new(),
            added_ticks: Vec::new(),
            mutated_ticks: Vec::new(),
            added_bits: BitSet::default(),
            changed_bits: BitSet::default(),
            component_type_id: id,
        }
    }
    
    fn len(&self) -> usize {
        self.chunks.iter().map(|c| c.len()).sum()
    }
}

impl<T: Component> ComponentColumn for TypedColumn<T> {
    #[inline] fn as_any(&self) -> &dyn Any { self }
    #[inline] fn as_any_mut(&mut self) -> &mut dyn Any { self }
    #[inline] fn component_type_id(&self) -> ComponentTypeId { self.component_type_id }
    #[inline] fn len(&self) -> usize { self.len() }
    
    #[inline]
    unsafe fn get_ptr(&self, index: usize) -> NonNull<u8> {
        let chunk_idx = index / CHUNK_SIZE;
        let offset = index % CHUNK_SIZE;
        NonNull::new_unchecked(self.chunks[chunk_idx].as_ptr().add(offset) as *mut u8)
    }

    #[inline]
    unsafe fn get_tick_ptr(&self, index: usize) -> NonNull<u32> {
        let chunk_idx = index / CHUNK_SIZE;
        let offset = index % CHUNK_SIZE;
        NonNull::new_unchecked(self.mutated_ticks[chunk_idx].as_ptr().add(offset) as *mut u32)
    }

    #[inline] fn added_bits(&self) -> &BitSet { &self.added_bits }
    #[inline] fn changed_bits(&self) -> &BitSet { &self.changed_bits }
    fn clear_change_bits(&mut self) { self.added_bits = BitSet::default(); self.changed_bits = BitSet::default(); }

    #[inline]
    unsafe fn push_raw(&mut self, bytes: *const u8, tick: u32) {
        if self.chunks.is_empty() || self.chunks.last().unwrap().len() == CHUNK_SIZE {
            self.chunks.push(Vec::with_capacity(CHUNK_SIZE));
            self.added_ticks.push(Vec::with_capacity(CHUNK_SIZE));
            self.mutated_ticks.push(Vec::with_capacity(CHUNK_SIZE));
        }
        let value = std::ptr::read(bytes as *const T);
        let index = self.len();
        self.chunks.last_mut().unwrap().push(value);
        self.added_ticks.last_mut().unwrap().push(tick);
        self.mutated_ticks.last_mut().unwrap().push(tick);
        self.added_bits.set(index as u32);
        self.changed_bits.set(index as u32);
    }

    unsafe fn transfer_out(&mut self, _index: usize, _dst: &mut dyn ComponentColumn, _tick: u32) { /* Omitted for brevity */ }
    unsafe fn swap_remove_drop(&mut self, _index: usize) { /* Omitted for brevity */ }
    unsafe fn swap_remove_read(&mut self, _index: usize) { /* Omitted for brevity */ }
    
    unsafe fn clear(&mut self) {
        self.chunks.clear();
        self.added_ticks.clear();
        self.mutated_ticks.clear();
        self.clear_change_bits();
    }
    
    fn reserve(&mut self, additional: usize) {
        let remaining = self.chunks.last().map_or(0, |c| CHUNK_SIZE - c.len());
        if additional > remaining {
            let new_chunks_needed = (additional - remaining + CHUNK_SIZE - 1) / CHUNK_SIZE;
            for _ in 0..new_chunks_needed {
                self.chunks.push(Vec::with_capacity(CHUNK_SIZE));
                self.added_ticks.push(Vec::with_capacity(CHUNK_SIZE));
                self.mutated_ticks.push(Vec::with_capacity(CHUNK_SIZE));
            }
        }
    }
}

pub(crate) fn make_column_fn<T: Component>(id: ComponentTypeId) -> Box<dyn ComponentColumn> {
    Box::new(TypedColumn::<T>::new(id))
}

pub struct ComponentDescriptor<'w, T: Component> {
    pub(crate) world: &'w mut World,
    pub(crate) _marker: std::marker::PhantomData<T>,
}

impl<'w, T: Component> ComponentDescriptor<'w, T> {
    pub fn storage_type(&mut self, storage: StorageType) -> &mut Self {
        let id = self.world.component_id::<T>().unwrap();
        self.world.component_infos[id.get() as usize - 1].storage = storage;
        self
    }
    pub fn on_add<F: Fn(&mut World, Entity) + Send + Sync + 'static>(&mut self, f: F) -> &mut Self {
        let id = self.world.component_id::<T>().unwrap();
        self.world.component_infos[id.get() as usize - 1].on_add = Some(Box::new(f));
        self
    }
    pub fn on_insert<F: Fn(&mut World, Entity) + Send + Sync + 'static>(&mut self, f: F) -> &mut Self {
        let id = self.world.component_id::<T>().unwrap();
        self.world.component_infos[id.get() as usize - 1].on_insert = Some(Box::new(f));
        self
    }
    pub fn on_replace<F: Fn(&mut World, Entity) + Send + Sync + 'static>(&mut self, f: F) -> &mut Self {
        let id = self.world.component_id::<T>().unwrap();
        self.world.component_infos[id.get() as usize - 1].on_replace = Some(Box::new(f));
        self
    }
    pub fn on_remove<F: Fn(&mut World, Entity) + Send + Sync + 'static>(&mut self, f: F) -> &mut Self {
        let id = self.world.component_id::<T>().unwrap();
        self.world.component_infos[id.get() as usize - 1].on_remove = Some(Box::new(f));
        self
    }
}
