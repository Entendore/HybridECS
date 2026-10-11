use core::num::{NonZeroU32, NonZeroU64};
use std::fmt;

pub type ComponentTypeId = NonZeroU32;
pub type ArchetypeId = NonZeroU32;

#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Entity(u64);

impl Entity {
    pub const PLACEHOLDER: Entity = Entity(u64::MAX);
    #[inline] pub fn id(&self) -> u32 { self.0 as u32 }
    #[inline] pub fn generation(&self) -> u32 { (self.0 >> 32) as u32 }
    #[inline] pub(crate) fn new(id: u32, gen: u32) -> Self { Self((id as u64) | ((gen as u64) << 32)) }
}

impl fmt::Display for Entity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { write!(f, "{}v{}", self.id(), self.generation()) }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct EntityLocation(NonZeroU64);

impl EntityLocation {
    #[inline]
    pub(crate) fn new(arch: ArchetypeId, idx: u32) -> Self {
        Self(NonZeroU64::new(((arch.get() as u64) << 32) | idx as u64).unwrap())
    }
    #[inline] pub(crate) fn archetype(&self) -> ArchetypeId { NonZeroU32::new((self.0.get() >> 32) as u32).unwrap() }
    #[inline] pub(crate) fn index(&self) -> u32 { self.0.get() as u32 }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Tick(pub(crate) u32);
