use crate::component::Mut;
use crate::entity::Tick;
use crate::private;
use crate::world::World;
use core::fmt;
use core::marker::PhantomData;
use core::num::NonZeroU32;
use core::ops::{Deref, DerefMut};
use std::any::TypeId;
use std::collections::HashMap;

pub trait Resource: private::SealedResource {}
impl<T: Send + Sync + 'static> private::SealedResource for T {}
impl<T: Send + Sync + 'static> Resource for T {}

#[repr(transparent)]
pub struct ResourceId(NonZeroU32);

impl ResourceId {
    #[inline] pub(crate) fn new(id: NonZeroU32) -> Self { Self(id) }
    #[inline] pub(crate) fn index(&self) -> usize { (self.0.get() - 1) as usize }
}

impl fmt::Debug for ResourceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { f.debug_tuple("ResourceId").field(&self.0).finish() }
}

pub struct ResourceHooks {
    pub on_insert: Option<Box<dyn Fn(&mut World) + Send + Sync>>,
    pub on_remove: Option<Box<dyn Fn(&mut World) + Send + Sync>>,
}

impl Default for ResourceHooks {
    fn default() -> Self { Self { on_insert: None, on_remove: None } }
}

pub struct NonSendMut<'w, T: ?Sized + 'w> {
    pub(crate) value: &'w mut T,
    _marker: PhantomData<*const ()>,
}

impl<'w, T: ?Sized> Deref for NonSendMut<'w, T> {
    type Target = T;
    #[inline] fn deref(&self) -> &Self::Target { self.value }
}
impl<'w, T: ?Sized> DerefMut for NonSendMut<'w, T> {
    #[inline] fn deref_mut(&mut self) -> &mut Self::Target { self.value }
}
impl<'w, T: ?Sized> NonSendMut<'w, T> {
    #[inline] pub fn reborrow(&mut self) -> NonSendMut<'_, T> { NonSendMut { value: self.value, _marker: PhantomData } }
}

pub struct Ref<'a, T: ?Sized + 'a> {
    pub(crate) value: &'a T,
    pub(crate) added_tick: u32,
    pub(crate) last_change_tick: u32,
}

impl<'a, T: ?Sized> Ref<'a, T> {
    #[inline] pub fn is_added(&self) -> bool { self.added_tick > self.last_change_tick }
    pub fn map<U: ?Sized, F: FnOnce(&T) -> &U>(orig: Ref<'a, T>, f: F) -> Ref<'a, U> {
        Ref { value: f(orig.value), added_tick: orig.added_tick, last_change_tick: orig.last_change_tick }
    }
}

impl<'a, T: ?Sized> Clone for Ref<'a, T> {
    fn clone(&self) -> Self { Ref { value: self.value, added_tick: self.added_tick, last_change_tick: self.last_change_tick } }
}

impl<T: ?Sized> Deref for Ref<'_, T> {
    type Target = T;
    #[inline] fn deref(&self) -> &Self::Target { self.value }
}

#[repr(transparent)]
pub struct Res<'w, T: Resource> { pub(crate) inner: Ref<'w, T> }

impl<'w, T: Resource> Res<'w, T> {
    #[inline] pub fn is_added(&self) -> bool { self.inner.is_added() }
}
impl<'w, T: Resource> Deref for Res<'w, T> {
    type Target = T;
    #[inline] fn deref(&self) -> &Self::Target { &self.inner }
}

#[repr(transparent)]
pub struct ResMut<'w, T: Resource> { pub(crate) inner: Mut<'w, T> }

impl<'w, T: Resource> ResMut<'w, T> {
    #[inline] pub fn is_changed(&self) -> bool { self.inner.is_changed() }
    #[inline] pub fn is_added(&self) -> bool { self.inner.last_change_tick == 0 }
    #[inline] pub fn last_changed(&self) -> Tick { self.inner.last_changed() }
    #[inline] pub fn reborrow(&mut self) -> ResMut<'_, T> { ResMut { inner: self.inner.reborrow() } }
}
impl<'w, T: Resource> Deref for ResMut<'w, T> {
    type Target = T;
    #[inline] fn deref(&self) -> &Self::Target { &self.inner }
}
impl<'w, T: Resource> DerefMut for ResMut<'w, T> {
    #[inline] fn deref_mut(&mut self) -> &mut Self::Target { &mut self.inner }
}
