use crate::bitset::BitSet;
use crate::component::ComponentColumn;
use crate::entity::{ArchetypeId, ComponentTypeId, Entity};

pub struct Archetype {
    pub(crate) id: ArchetypeId,
    pub(crate) signature: BitSet,
    pub(crate) columns: Vec<Option<Box<dyn ComponentColumn>>>,
    pub(crate) entities: Vec<Entity>,
}

impl Archetype {
    pub fn new(id: ArchetypeId, num_ct: usize) -> Self {
        let mut columns = Vec::with_capacity(num_ct);
        columns.resize_with(num_ct, || None);
        Archetype { id, signature: BitSet::default(), columns, entities: Vec::new() }
    }

    #[inline] pub fn len(&self) -> usize { self.entities.len() }
    #[inline] pub fn is_empty(&self) -> bool { self.entities.is_empty() }
    
    #[inline] pub(crate) fn column(&self, id: ComponentTypeId) -> Option<&dyn ComponentColumn> {
        self.columns.get(id.get() as usize - 1)?.as_deref()
    }
    #[inline] pub(crate) fn column_mut(&mut self, id: ComponentTypeId) -> Option<&mut (dyn ComponentColumn)> {
        self.columns.get_mut(id.get() as usize - 1)?.as_deref_mut()
    }

    pub(crate) fn reserve(&mut self, additional: usize) {
        for col in self.columns.iter_mut().flatten() { col.reserve(additional); }
        self.entities.reserve(additional);
    }
}
