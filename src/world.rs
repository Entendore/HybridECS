use crate::archetype::Archetype;
use crate::bitset::BitSet;
use crate::bundle::Bundle;
use crate::component::{make_column_fn, Component, ComponentColumn, ComponentDescriptor, ComponentInfo, Mut, StorageType};
use crate::entity::{ArchetypeId, ComponentTypeId, Entity, EntityLocation, Tick};
use crate::error::FetchEntityError;
use crate::events::{Event, EventReader, EventWriter, Events};
use crate::query::{Query, QueryFilter, QueryIter, QueryMut, QueryMutIter, QueryState};
use crate::reflect::TypeRegistry;
use crate::resource::{NonSendMut, Ref, Res, ResMut, Resource, ResourceHooks, ResourceId};
use rayon::prelude::*;
use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::fmt;

struct ResourceData { data: Box<dyn Any + Send + Sync>, tick: u32 }
struct NonSendResourceData { data: Box<dyn Any>, tick: u32 }
pub(crate) struct BundleInfo { pub(crate) ids: Vec<ComponentTypeId>, pub(crate) signature: BitSet }

pub struct World {
    pub(crate) generations: Vec<u32>,
    pub(crate) locations: Vec<u64>, // 0 = dead/free, >0 = EntityLocation(NonZeroU64)
    pub(crate) free_head: u32,
    pub(crate) component_infos: Vec<ComponentInfo>,
    pub(crate) type_id_to_component_id: HashMap<TypeId, ComponentTypeId>,
    pub(crate) archetypes: Vec<Archetype>,
    pub(crate) archetype_by_signature: HashMap<BitSet, ArchetypeId>,
    pub(crate) archetype_generation: usize,
    pub(crate) add_edges: HashMap<(ArchetypeId, ComponentTypeId), ArchetypeId>,
    pub(crate) remove_edges: HashMap<(ArchetypeId, ComponentTypeId), ArchetypeId>,
    pub(crate) resource_registry: HashMap<TypeId, ResourceId>,
    pub(crate) resources: Vec<Option<ResourceData>>,
    pub(crate) non_send_resources: Vec<Option<NonSendResourceData>>,
    pub(crate) resource_added_ticks: HashMap<TypeId, Tick>,
    pub(crate) removed_resources: Vec<TypeId>,
    pub(crate) resource_hooks: HashMap<TypeId, ResourceHooks>,
    pub(crate) removed_components: HashMap<TypeId, Vec<Entity>>,
    pub(crate) bundle_infos: HashMap<TypeId, BundleInfo>,
    pub(crate) current_tick: Tick,
    pub(crate) last_change_tick: Tick,
}

impl Default for World { fn default() -> Self { Self::new() } }

impl World {
    pub fn new() -> Self {
        let mut world = World {
            generations: Vec::new(), locations: Vec::new(), free_head: u32::MAX,
            component_infos: Vec::new(), type_id_to_component_id: HashMap::new(),
            archetypes: Vec::new(), archetype_by_signature: HashMap::new(), archetype_generation: 0,
            add_edges: HashMap::new(), remove_edges: HashMap::new(),
            resource_registry: HashMap::new(), resources: Vec::new(), non_send_resources: Vec::new(),
            resource_added_ticks: HashMap::new(), removed_resources: Vec::new(), resource_hooks: HashMap::new(),
            removed_components: HashMap::new(), bundle_infos: HashMap::new(),
            current_tick: Tick(1), last_change_tick: Tick(0),
        };
        world.get_or_create_archetype(BitSet::default());
        world
    }

    #[inline] pub fn increment_tick(&mut self) {
        self.last_change_tick = self.current_tick;
        self.current_tick.0 += 1;
        for arch in &mut self.archetypes {
            for col in arch.columns.iter_mut().flatten() { col.clear_change_bits(); }
        }
    }

    pub fn clear(&mut self) {
        for arch in &mut self.archetypes {
            for col in arch.columns.iter_mut().flatten() { unsafe { col.clear(); } }
            arch.entities.clear();
        }
        for i in 0..self.generations.len() {
            self.generations[i] = self.generations[i].wrapping_add(1);
            self.locations[i] = 0;
            self.free_head = i as u32;
        }
        self.removed_components.clear();
        self.resource_added_ticks.clear();
        self.removed_resources.clear();
        self.resources.fill(None);
        self.non_send_resources.fill(None);
    }

    pub(crate) fn register_component<T: Component>(&mut self) -> ComponentTypeId {
        let type_id = TypeId::of::<T>();
        if let Some(&id) = self.type_id_to_component_id.get(&type_id) { return id; }
        let id = ComponentTypeId::new(self.component_infos.len() as u32 + 1).unwrap();
        self.component_infos.push(ComponentInfo {
            type_id, storage: StorageType::Archetype, make_column: make_column_fn::<T>,
            on_add: None, on_insert: None, on_replace: None, on_remove: None,
        });
        self.type_id_to_component_id.insert(type_id, id);
        for arch in &mut self.archetypes {
            if arch.columns.len() < id.get() as usize { arch.columns.resize(id.get() as usize, None); }
        }
        id
    }

    pub fn component<T: Component>(&mut self) -> ComponentDescriptor<T> {
        let id = self.register_component::<T>();
        ComponentDescriptor { world: self, _marker: std::marker::PhantomData }
    }

    #[inline] pub fn component_id<T: Component>(&self) -> Option<ComponentTypeId> {
        self.type_id_to_component_id.get(&TypeId::of::<T>()).copied()
    }

    pub(crate) fn get_or_init_bundle_info<B: Bundle>(&mut self) -> &BundleInfo {
        let type_id = TypeId::of::<B>();
        if !self.bundle_infos.contains_key(&type_id) {
            let mut ids = Vec::new();
            B::component_ids(self, &mut ids);
            let mut sig = BitSet::default();
            for id in &ids { sig.set(id.get()); }
            self.bundle_infos.insert(type_id, BundleInfo { ids, signature: sig });
        }
        self.bundle_infos.get(&type_id).unwrap()
    }

    pub(crate) fn get_or_create_archetype(&mut self, signature: BitSet) -> ArchetypeId {
        if let Some(&id) = self.archetype_by_signature.get(&signature) { return id; }
        let id = ArchetypeId::new(self.archetypes.len() as u32 + 1).unwrap();
        let num_ct = self.component_infos.len();
        let mut archetype = Archetype::new(id, num_ct);
        archetype.signature = signature.clone();
        for (i, cid) in self.component_infos.iter().enumerate() {
            let cid_val = (i as u32) + 1;
            if signature.contains(cid_val) { archetype.columns[i] = Some((cid.make_column)(ComponentTypeId::new(cid_val).unwrap())); }
        }
        self.archetypes.push(archetype);
        self.archetype_by_signature.insert(signature, id);
        self.archetype_generation += 1;
        id
    }

    fn register_resource<T: 'static>(&mut self) -> ResourceId {
        let type_id = TypeId::of::<T>();
        if let Some(&id) = self.resource_registry.get(&type_id) { return id; }
        let id = ResourceId::new(NonZeroU32::new(self.resources.len() as u32 + 1).unwrap());
        self.resources.push(None);
        self.non_send_resources.push(None);
        self.resource_registry.insert(type_id, id);
        id
    }

    #[inline] pub(crate) fn resource_id<T: Resource>(&self) -> Option<ResourceId> {
        self.resource_registry.get(&TypeId::of::<T>()).copied()
    }

    pub fn register_resource_hooks<R: Resource, F: FnOnce(&mut ResourceHooks)>(&mut self, f: F) {
        let type_id = TypeId::of::<R>();
        let hooks = self.resource_hooks.entry(type_id).or_default();
        f(hooks);
    }

    pub fn insert_resource<R: Resource>(&mut self, resource: R) {
        let type_id = TypeId::of::<R>();
        let id = self.register_resource::<R>();
        let idx = id.index();
        self.resources[idx] = Some(ResourceData { data: Box::new(resource), tick: self.current_tick.0 });
        self.resource_added_ticks.insert(type_id, self.current_tick);
        if let Some(hook) = self.resource_hooks.get(&type_id).and_then(|h| h.on_insert.as_ref()) { hook(self); }
    }

    pub fn remove_resource<R: Resource>(&mut self) -> Option<R> {
        let type_id = TypeId::of::<R>();
        let id = self.resource_registry.get(&type_id).copied()?;
        let idx = id.index();
        let res = self.resources[idx].take()?.data.downcast::<R>().ok().map(|b| *b);
        if res.is_some() {
            self.removed_resources.push(type_id);
            if let Some(hook) = self.resource_hooks.get(&type_id).and_then(|h| h.on_remove.as_ref()) { hook(self); }
        }
        res
    }

    #[inline] pub fn get_resource<R: Resource>(&self) -> Option<Res<'_, R>> {
        let id = self.resource_id::<R>()?;
        let res = self.resources[id.index()].as_ref()?;
        Some(Res { inner: Ref {
            value: res.data.downcast_ref::<R>()?,
            added_tick: self.resource_added_ticks.get(&TypeId::of::<R>()).map_or(0, |t| t.0),
            last_change_tick: self.last_change_tick.0,
        }})
    }

    pub fn get_resource_mut<R: Resource>(&mut self) -> Option<ResMut<'_, R>> {
        let id = self.resource_id::<R>()?;
        let res = self.resources[id.index()].as_mut()?;
        Some(ResMut { inner: Mut {
            value: res.data.downcast_mut::<R>()?,
            tick: &mut res.tick,
            changed_bits: None, index: 0,
            current_tick: self.current_tick, last_change_tick: self.last_change_tick,
        }})
    }

    #[track_caller] pub fn resource<R: Resource>(&self) -> &R { self.get_resource::<R>().expect("Resource does not exist") }
    #[track_caller] pub fn resource_mut<R: Resource>(&mut self) -> ResMut<'_, R> { self.get_resource_mut::<R>().expect("Resource does not exist") }

    pub fn resource_added<R: Resource>(&self) -> bool { self.resource_added_ticks.get(&TypeId::of::<R>()).map_or(false, |t| *t > self.last_change_tick) }
    pub fn resource_removed<R: Resource>(&mut self) -> bool {
        let type_id = TypeId::of::<R>();
        if let Some(idx) = self.removed_resources.iter().position(|t| *t == type_id) { self.removed_resources.swap_remove(idx); true } else { false }
    }
    pub fn resource_changed<R: Resource>(&self) -> bool {
        let id = self.resource_id::<R>().ok();
        id.map_or(false, |id| self.resources[id.index()].as_ref().map_or(false, |r| r.tick > self.last_change_tick.0))
    }

    pub fn init_resource<R: Resource + Default>(&mut self) -> &mut Self {
        let type_id = TypeId::of::<R>();
        let id = self.register_resource::<R>();
        let idx = id.index();
        if self.resources[idx].is_none() {
            self.resources[idx] = Some(ResourceData { data: Box::new(R::default()), tick: self.current_tick.0 });
            self.resource_added_ticks.insert(type_id, self.current_tick);
        }
        self
    }

    pub fn get_resource_or_default<R: Resource + Default>(&mut self) -> ResMut<'_, R> {
        self.init_resource::<R>();
        self.get_resource_mut::<R>().unwrap()
    }

    pub fn get_or_insert<R: Resource, F: FnOnce() -> R>(&mut self, f: F) -> ResMut<'_, R> {
        let type_id = TypeId::of::<R>();
        let id = self.register_resource::<R>();
        let idx = id.index();
        if self.resources[idx].is_none() {
            self.resources[idx] = Some(ResourceData { data: Box::new(f()), tick: self.current_tick.0 });
            self.resource_added_ticks.insert(type_id, self.current_tick);
        }
        self.get_resource_mut::<R>().unwrap()
    }

    pub fn resource_scope<R: Resource, U>(&mut self, f: impl FnOnce(&mut World, &mut R) -> U) -> U {
        let mut resource = self.remove_resource::<R>().expect("Resource does not exist");
        let result = f(self, &mut resource);
        self.insert_resource(resource);
        result
    }

    pub fn insert_non_send_resource<T: 'static>(&mut self, resource: T) {
        let type_id = TypeId::of::<T>();
        let id = self.register_resource::<T>();
        let idx = id.index();
        self.non_send_resources[idx] = Some(NonSendResourceData { data: Box::new(resource), tick: self.current_tick.0 });
        self.resource_added_ticks.insert(type_id, self.current_tick);
    }

    pub fn get_non_send_resource<T: 'static>(&self) -> Option<&T> {
        let id = self.resource_id::<T>()?;
        self.non_send_resources[id.index()].as_ref()?.data.downcast_ref::<T>()
    }

    pub fn get_non_send_resource_mut<T: 'static>(&mut self) -> Option<NonSendMut<'_, T>> {
        let id = self.resource_id::<T>()?;
        let res = self.non_send_resources[id.index()].as_mut()?;
        res.tick = self.current_tick.0;
        let val = res.data.downcast_mut::<T>()?;
        Some(NonSendMut { value: val, _marker: std::marker::PhantomData })
    }

    pub fn remove_non_send_resource<T: 'static>(&mut self) -> Option<T> {
        let type_id = TypeId::of::<T>();
        let id = self.resource_registry.get(&type_id).copied()?;
        let idx = id.index();
        let res = self.non_send_resources[idx].take()?.data.downcast::<T>().ok().map(|b| *b);
        if res.is_some() { self.removed_resources.push(type_id); }
        res
    }

    pub fn init_events<T: Event>(&mut self) -> &mut Self { self.init_resource::<Events<T>>() }
    pub fn get_event_writer<T: Event>(&mut self) -> Option<EventWriter<'_, T>> { self.get_resource_mut::<Events<T>>().map(|events| EventWriter { events: events.inner }) }
    pub fn get_event_reader<T: Event>(&self) -> Option<EventReader<'_, T>> { self.get_resource::<Events<T>>().map(|events| EventReader { events: &events.inner } ) }
    pub fn flush_events<T: Event>(&mut self) { if let Some(mut events) = self.get_resource_mut::<Events<T>>() { events.inner.flush(); } }

    #[inline] pub fn len(&self) -> usize { self.locations.iter().filter(|l| **l != 0).count() }
    #[inline] pub fn is_empty(&self) -> bool { self.len() == 0 }
    pub fn iter_entities(&self) -> impl Iterator<Item = Entity> + '_ {
        self.locations.iter().enumerate().filter_map(|(i, loc)| {
            if *loc != 0 { Some(Entity::new(i as u32, self.generations[i])) } else { None }
        })
    }
    pub fn entities(&self) -> impl Iterator<Item = Entity> + '_ { self.iter_entities() }

    #[must_use] pub fn build_entity(&mut self) -> EntityBuilder<'_> {
        let arch_id = ArchetypeId::new(1).unwrap();
        let entity = self.allocate_entity(arch_id);
        EntityBuilder { world: self, entity }
    }

    pub fn spawn_empty(&mut self) -> Entity { let arch_id = ArchetypeId::new(1).unwrap(); self.allocate_entity(arch_id) }

    pub fn spawn<B: Bundle>(&mut self, bundle: B) -> Entity {
        let info = self.get_or_init_bundle_info::<B>();
        let arch_id = self.get_or_create_archetype(info.signature.clone());
        let entity = self.allocate_entity(arch_id);
        let mut hooks_to_fire = Vec::new();
        for &id in &info.ids {
            if let Some(hook) = self.component_infos[id.get() as usize - 1].on_add.as_ref() { hooks_to_fire.push(hook); }
        }
        let arch = &mut self.archetypes[arch_id.get() as usize - 1];
        unsafe { bundle.put(arch, &info.ids, self.current_tick.0); }
        for hook in hooks_to_fire { hook(self, entity); }
        entity
    }

    pub(crate) fn allocate_entity(&mut self, arch_id: ArchetypeId) -> Entity {
        let (id, generation) = if self.free_head != u32::MAX {
            let id = self.free_head;
            self.free_head = self.locations[id as usize] as u32;
            let prev_gen = self.generations[id as usize];
            (id, prev_gen.wrapping_add(1))
        } else {
            let id = self.generations.len() as u32;
            self.generations.push(0);
            self.locations.push(0);
            (id, 0)
        };
        let entity = Entity::new(id, generation);
        let arch = &mut self.archetypes[arch_id.get() as usize - 1];
        let index = arch.entities.len() as u32;
        arch.entities.push(entity);
        self.generations[id as usize] = generation;
        self.locations[id as usize] = EntityLocation::new(arch_id, index).0.get();
        entity
    }

    pub fn despawn(&mut self, entity: Entity) -> bool {
        let id = entity.id() as usize;
        if id >= self.generations.len() || self.generations[id] != entity.generation() { return false; }
        let loc_raw = self.locations[id];
        if loc_raw == 0 { return false; }
        let loc = EntityLocation(core::num::NonZeroU64::new(loc_raw).unwrap());
        
        if let Some(parent) = self.get::<crate::hierarchy::Parent>(entity).cloned() {
            if let Some(mut children) = self.get_mut::<crate::hierarchy::Children>(parent.0) { children.0.retain(|&c| c != entity); }
        }

        let arch = &mut self.archetypes[loc.archetype().get() as usize - 1];
        let sig = arch.signature.clone();
        let mut hooks_to_fire = Vec::new();
        for cid in sig.iter() {
            let col_idx = cid as usize - 1;
            let type_id = self.component_infos[col_idx].type_id;
            if let Some(hook) = self.component_infos[col_idx].on_remove.as_ref() { hooks_to_fire.push(hook); }
            self.removed_components.entry(type_id).or_default().push(entity);
            unsafe { arch.columns[col_idx].as_mut().unwrap().swap_remove_drop(loc.index() as usize); }
        }
        arch.entities.swap_remove(loc.index() as usize);
        if loc.index() as usize < arch.entities.len() {
            let swapped = arch.entities[loc.index() as usize];
            self.set_entity_location(swapped, EntityLocation::new(loc.archetype(), loc.index()));
        }
        self.locations[id] = self.free_head as u64;
        self.free_head = entity.id();
        self.generations[id] = entity.generation().wrapping_add(1);
        for hook in hooks_to_fire { hook(self, entity); }
        true
    }

    pub fn try_despawn(&mut self, entity: Entity) -> Result<(), FetchEntityError> {
        if self.despawn(entity) { Ok(()) } else { Err(FetchEntityError::NoSuchEntity) }
    }

    pub fn insert_bundle<B: Bundle>(&mut self, entity: Entity, bundle: B) {
        let info = self.get_or_init_bundle_info::<B>();
        let loc = match self.entity_location(entity) { Some(l) => l, None => return };
        let src_arch_id = loc.archetype();
        let mut new_sig = self.archetypes[src_arch_id.get() as usize - 1].signature.clone();
        for id in &info.ids { new_sig.set(id.get()); }
        
        let mut add_hooks = Vec::new();
        let mut insert_hooks = Vec::new();
        let mut replace_hooks = Vec::new();
        for &id in &info.ids {
            let info_data = &self.component_infos[id.get() as usize - 1];
            if self.archetypes[src_arch_id.get() as usize - 1].signature.contains(id.get()) {
                if let Some(hook) = info_data.on_replace.as_ref() { replace_hooks.push(hook); }
                if let Some(hook) = info_data.on_insert.as_ref() { insert_hooks.push(hook); }
            } else {
                if let Some(hook) = info_data.on_add.as_ref() { add_hooks.push(hook); }
                if let Some(hook) = info_data.on_insert.as_ref() { insert_hooks.push(hook); }
            }
        }
        for hook in &replace_hooks { hook(self, entity); }

        if new_sig == self.archetypes[src_arch_id.get() as usize - 1].signature {
            let arch = &mut self.archetypes[src_arch_id.get() as usize - 1];
            unsafe { bundle.put(arch, &info.ids, self.current_tick.0); }
            for hook in insert_hooks { hook(self, entity); }
            return;
        }
        let new_arch_id = self.get_or_create_archetype(new_sig);
        self.move_entity(entity, loc, new_arch_id, Some((info.ids.clone(), bundle)));
        for hook in add_hooks { hook(self, entity); }
        for hook in insert_hooks { hook(self, entity); }
    }

    #[inline] pub fn insert<T: Component>(&mut self, entity: Entity, component: T) { self.insert_bundle(entity, (component,)) }
    #[track_caller] pub fn try_insert<T: Component>(&mut self, entity: Entity, component: T) -> Result<(), FetchEntityError> {
        if !self.is_alive(entity) { return Err(FetchEntityError::NoSuchEntity); }
        self.insert(entity, component);
        Ok(())
    }

    pub fn remove<T: Component>(&mut self, entity: Entity) -> Option<T> {
        let cid = self.register_component::<T>();
        let loc = self.entity_location(entity)?;
        let src_arch_id = loc.archetype();
        if !self.archetypes[src_arch_id.get() as usize - 1].signature.contains(cid.get()) { return None; }
        let new_arch_id = *self.remove_edges.entry((src_arch_id, cid)).or_insert_with(|| {
            let mut new_sig = self.archetypes[src_arch_id.get() as usize - 1].signature.clone();
            new_sig.clear(cid.get());
            self.get_or_create_archetype(new_sig)
        });
        let val = unsafe {
            let arch = &mut self.archetypes[src_arch_id.get() as usize - 1];
            let col = arch.column_mut(cid).unwrap();
            let ptr = col.get_ptr(loc.index() as usize);
            ptr.cast::<T>().as_ptr().read()
        };
        self.move_entity_without(entity, loc, new_arch_id, cid);
        self.removed_components.entry(TypeId::of::<T>()).or_default().push(entity);
        if let Some(hook) = self.component_infos[cid.get() as usize - 1].on_remove.as_ref() { hook(self, entity); }
        Some(val)
    }

    pub fn removed_components<T: Component>(&mut self) -> impl Iterator<Item = Entity> + '_ {
        self.removed_components.remove(&TypeId::of::<T>()).unwrap_or_default().into_iter()
    }

    fn move_entity_without(&mut self, entity: Entity, loc: EntityLocation, dst_arch_id: ArchetypeId, skip_cid: ComponentTypeId) {
        let src_arch_id = loc.archetype();
        let index = loc.index() as usize;
        let src_sig = self.archetypes[src_arch_id.get() as usize - 1].signature.clone();
        let arch_ptr = self.archetypes.as_mut_ptr();
        let mut updates = Vec::new();
        unsafe {
            let src = &mut *arch_ptr.add(src_arch_id.get() as usize - 1);
            let dst = &mut *arch_ptr.add(dst_arch_id.get() as usize - 1);
            for cid in src_sig.iter() {
                let c = ComponentTypeId::new(cid).unwrap();
                let col_idx = cid as usize - 1;
                if c == skip_cid { src.columns[col_idx].as_mut().unwrap().swap_remove_read(index); }
                else {
                    let src_col = src.columns[col_idx].as_mut().unwrap();
                    let dst_col = dst.columns[col_idx].as_mut().unwrap();
                    src_col.transfer_out(index, dst_col, self.current_tick.0);
                }
            }
            src.entities.swap_remove(index);
            if index < src.entities.len() {
                let swapped = src.entities[index];
                updates.push((swapped, EntityLocation::new(src_arch_id, index as u32)));
            }
            let new_idx = dst.entities.len() as u32;
            dst.entities.push(entity);
            updates.push((entity, EntityLocation::new(dst_arch_id, new_idx)));
        }
        for (e, l) in updates { self.set_entity_location(e, l); }
    }

    fn move_entity<B: Bundle>(&mut self, entity: Entity, loc: EntityLocation, dst_arch_id: ArchetypeId, new_bundle: Option<(Vec<ComponentTypeId>, B)>) {
        let src_arch_id = loc.archetype();
        let index = loc.index() as usize;
        let src_sig = self.archetypes[src_arch_id.get() as usize - 1].signature.clone();
        let arch_ptr = self.archetypes.as_mut_ptr();
        let mut updates = Vec::new();
        unsafe {
            let src = &mut *arch_ptr.add(src_arch_id.get() as usize - 1);
            let dst = &mut *arch_ptr.add(dst_arch_id.get() as usize - 1);
            for cid in src_sig.iter() {
                let c = ComponentTypeId::new(cid).unwrap();
                let col_idx = cid as usize - 1;
                if let Some((ids, _)) = &new_bundle {
                    if ids.contains(&c) { src.columns[col_idx].as_mut().unwrap().swap_remove_drop(index); continue; }
                }
                let src_col = src.columns[col_idx].as_mut().unwrap();
                let dst_col = dst.columns[col_idx].as_mut().unwrap();
                src_col.transfer_out(index, dst_col, self.current_tick.0);
            }
            if let Some((ids, bundle)) = new_bundle { bundle.put(dst, &ids, self.current_tick.0); }
            src.entities.swap_remove(index);
            if index < src.entities.len() {
                let swapped = src.entities[index];
                updates.push((swapped, EntityLocation::new(src_arch_id, index as u32)));
            }
            let new_idx = dst.entities.len() as u32;
            dst.entities.push(entity);
            updates.push((entity, EntityLocation::new(dst_arch_id, new_idx)));
        }
        for (e, l) in updates { self.set_entity_location(e, l); }
    }

    #[inline] pub fn get<T: Component>(&self, entity: Entity) -> Option<&T> {
        let loc = self.entity_location(entity)?;
        let cid = self.component_id::<T>()?;
        let arch = &self.archetypes[loc.archetype().get() as usize - 1];
        let col = arch.column(cid)?;
        unsafe { let ptr = col.get_ptr(loc.index() as usize); Some(&*ptr.cast::<T>().as_ptr()) }
    }

    pub fn get_mut<T: Component>(&mut self, entity: Entity) -> Option<Mut<'_, T>> {
        let loc = self.entity_location(entity)?;
        let cid = self.component_id::<T>()?;
        let storage = self.component_infos[cid.get() as usize - 1].storage;
        if storage == StorageType::Immutable { panic!("Cannot get mutable access to immutable component {}", std::any::type_name::<T>()); }
        let arch = &mut self.archetypes[loc.archetype().get() as usize - 1];
        let col = arch.column_mut(cid)?;
        let idx = loc.index() as usize;
        unsafe {
            let val_ptr = col.get_ptr(idx).cast::<T>();
            let tick_ptr = col.get_tick_ptr(idx).as_ptr();
            Some(Mut {
                value: &mut *val_ptr.as_ptr(), tick: &mut *tick_ptr,
                changed_bits: Some(col.changed_bits()), index: idx as u32,
                current_tick: self.current_tick, last_change_tick: self.last_change_tick,
            })
        }
    }

    #[track_caller] pub fn get_many_mut<T: Component>(&mut self, entities: &[Entity]) -> Vec<Option<Mut<'_, T>>> {
        use std::collections::HashSet;
        let mut seen = HashSet::new();
        for e in entities { if !seen.insert(*e) { panic!("Duplicate entity {:?} passed to get_many_mut", e); } }
        entities.iter().map(|e| self.get_mut::<T>(*e)).collect()
    }

    #[inline] pub fn contains_component<T: Component>(&self, entity: Entity) -> bool {
        match self.entity_location(entity) {
            Some(loc) => match self.component_id::<T>() {
                Some(cid) => self.archetypes[loc.archetype().get() as usize - 1].signature.contains(cid.get()),
                None => false,
            },
            None => false,
        }
    }

    #[inline] pub fn try_get<T: Component>(&self, entity: Entity) -> Result<&T, FetchEntityError> {
        let loc = self.entity_location(entity).ok_or(FetchEntityError::NoSuchEntity)?;
        let cid = self.component_id::<T>().ok_or(FetchEntityError::MissingComponent)?;
        let arch = &self.archetypes[loc.archetype().get() as usize - 1];
        let col = arch.column(cid).ok_or(FetchEntityError::MissingComponent)?;
        unsafe { let ptr = col.get_ptr(loc.index() as usize); Ok(&*ptr.cast::<T>().as_ptr()) }
    }

    #[inline] pub fn try_get_mut<T: Component>(&mut self, entity: Entity) -> Result<Mut<'_, T>, FetchEntityError> {
        let loc = self.entity_location(entity).ok_or(FetchEntityError::NoSuchEntity)?;
        let cid = self.component_id::<T>().ok_or(FetchEntityError::MissingComponent)?;
        let arch = &mut self.archetypes[loc.archetype().get() as usize - 1];
        let col = arch.column_mut(cid).ok_or(FetchEntityError::MissingComponent)?;
        let idx = loc.index() as usize;
        unsafe {
            let val_ptr = col.get_ptr(idx).cast::<T>();
            let tick_ptr = col.get_tick_ptr(idx).as_ptr();
            Ok(Mut { value: &mut *val_ptr.as_ptr(), tick: &mut *tick_ptr, changed_bits: None, index: idx as u32, current_tick: self.current_tick, last_change_tick: self.last_change_tick })
        }
    }

    #[inline] pub fn is_alive(&self, entity: Entity) -> bool { self.entity_location(entity).is_some() }
    #[inline] pub(crate) fn entity_location(&self, entity: Entity) -> Option<EntityLocation> {
        let id = entity.id() as usize;
        if id >= self.generations.len() { return None; }
        if self.generations[id] != entity.generation() { return None; }
        let loc_raw = self.locations[id];
        if loc_raw == 0 { return None; }
        Some(EntityLocation(core::num::NonZeroU64::new(loc_raw).unwrap()))
    }

    fn set_entity_location(&mut self, entity: Entity, location: EntityLocation) {
        let id = entity.id() as usize;
        if self.generations[id] == entity.generation() { self.locations[id] = location.0.get(); }
    }

    #[must_use] pub fn query<Q: Query, F: QueryFilter>(&self) -> QueryIter<'_, Q, F> {
        let mut state = QueryState::<Q, F>::new(self); state.update_archetypes(self); QueryIter::new(self, &state)
    }
    #[must_use] pub fn query_mut<Q: QueryMut, F: QueryFilter>(&mut self) -> QueryMutIter<'_, Q, F> {
        let mut state = QueryState::<Q, F>::new(self); state.update_archetypes(self); QueryMutIter::new(self, &state)
    }
    #[must_use] pub fn query_state<Q: Query, F: QueryFilter>(&self) -> QueryState<Q, F> {
        let mut state = QueryState::<Q, F>::new(self); state.update_archetypes(self); state
    }

    pub fn entity_ref(&self, entity: Entity) -> Option<EntityRef<'_>> {
        if self.is_alive(entity) { Some(EntityRef { world: self, entity }) } else { None }
    }
    #[track_caller] pub fn entity(&self, entity: Entity) -> EntityRef<'_> { self.entity_ref(entity).unwrap_or_else(|| panic!("Entity {} does not exist", entity)) }
    pub fn entity_mut(&mut self, entity: Entity) -> Option<EntityMut<'_>> {
        if self.is_alive(entity) { Some(EntityMut { world: self, entity }) } else { None }
    }
    #[track_caller] pub fn entity_mut_or_panic(&mut self, entity: Entity) -> EntityMut<'_> { self.entity_mut(entity).unwrap_or_else(|| panic!("Entity {} does not exist", entity)) }
    pub fn get_or_spawn(&mut self, entity: Entity) -> EntityMut<'_> {
        if self.is_alive(entity) { EntityMut { world: self, entity } }
        else {
            let arch_id = ArchetypeId::new(1).unwrap();
            let new_entity = self.allocate_entity(arch_id);
            EntityMut { world: self, entity: new_entity }
        }
    }

    pub fn commands_scope<U>(&mut self, f: impl FnOnce(&mut Commands) -> U) -> U {
        let mut queue = CommandQueue::new();
        let result = { let mut commands = Commands::new(self, &mut queue); f(&mut commands) };
        queue.apply(self);
        result
    }

    pub fn insert_batch<T: Component + Clone>(&mut self, entities: &[Entity], component: T) {
        for e in entities { self.insert(*e, component.clone()); }
    }
    pub fn despawn_batch(&mut self, entities: &[Entity]) { for e in entities { self.despawn(*e); } }
    pub fn children(&self, entity: Entity) -> Option<&crate::hierarchy::Children> { self.get::<crate::hierarchy::Children>(entity) }

    pub fn reserve_entities(&mut self, additional: usize) {
        self.generations.reserve(additional);
        self.locations.reserve(additional);
    }

    pub fn spawn_at(&mut self, entity: Entity) -> bool {
        let id = entity.id() as usize;
        if id < self.generations.len() {
            if self.generations[id] == entity.generation() && self.locations[id] != 0 { return false; }
        } else {
            self.generations.resize(id + 1, 0);
            self.locations.resize(id + 1, 0);
        }
        self.generations[id] = entity.generation();
        let arch_id = ArchetypeId::new(1).unwrap();
        let arch = &mut self.archetypes[arch_id.get() as usize - 1];
        let index = arch.entities.len() as u32;
        arch.entities.push(entity);
        self.locations[id] = EntityLocation::new(arch_id, index).0.get();
        true
    }

    pub fn insert_or_spawn_batch<I, B>(&mut self, iter: I) -> Vec<Entity>
    where I: IntoIterator<Item = (Option<Entity>, B)>, B: Bundle {
        iter.into_iter().map(|(entity, bundle)| {
            match entity {
                Some(e) if self.is_alive(e) => { self.insert_bundle(e, bundle); e }
                _ => self.spawn(bundle),
            }
        }).collect()
    }
}

impl<B: Bundle> Extend<B> for World {
    fn extend<T: IntoIterator<Item = B>>(&mut self, iter: T) { for b in iter { self.spawn(b); } }
}
impl<B: Bundle> FromIterator<B> for World {
    fn from_iter<T: IntoIterator<Item = B>>(iter: T) -> Self { let mut world = World::new(); world.extend(iter); world }
}

impl fmt::Display for World {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let alive = self.locations.iter().filter(|l| **l != 0).count();
        writeln!(f, "World {{")?;
        writeln!(f, "  alive_entities: {},", alive)?;
        writeln!(f, "  archetype_count: {},", self.archetypes.len())?;
        writeln!(f, "  component_types: {},", self.component_infos.len())?;
        writeln!(f, "  resource_count: {},", self.resources.iter().filter(|r| r.is_some()).count())?;
        writeln!(f, "  current_tick: {},", self.current_tick.0)?;
        write!(f, "}}")
    }
}

#[must_use] pub struct EntityBuilder<'w> { world: &'w mut World, entity: Entity }
impl<'w> EntityBuilder<'w> {
    #[inline] pub fn with<T: Component>(mut self, component: T) -> Self { self.world.insert(self.entity, component); self }
    #[inline] pub fn with_bundle<B: Bundle>(mut self, bundle: B) -> Self { self.world.insert_bundle(self.entity, bundle); self }
    #[inline] pub fn build(self) -> Entity { self.entity }
}

pub struct EntityRef<'w> { world: &'w World, entity: Entity }
impl<'w> EntityRef<'w> {
    #[inline] pub fn get<T: Component>(&self) -> Option<&T> { self.world.get(self.entity) }
    #[inline] pub fn contains<T: Component>(&self) -> bool { self.get::<T>().is_some() }
    #[inline] pub fn id(&self) -> Entity { self.entity }
}

impl fmt::Debug for EntityRef<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let loc = self.world.entity_location(self.entity);
        if let Some(loc) = loc {
            let arch = &self.world.archetypes[loc.archetype().get() as usize - 1];
            f.debug_struct("EntityRef").field("entity", &self.entity).field("archetype_id", &loc.archetype()).field("index", &loc.index()).field("signature", &arch.signature).finish()
        } else {
            f.debug_struct("EntityRef").field("entity", &self.entity).field("status", &"Despawned").finish()
        }
    }
}

pub struct EntityMut<'w> { world: &'w mut World, entity: Entity }
impl<'w> EntityMut<'w> {
    #[inline] pub fn get<T: Component>(&self) -> Option<&T> { self.world.get(self.entity) }
    #[inline] pub fn get_mut<T: Component>(&mut self) -> Option<Mut<'_, T>> { self.world.get_mut(self.entity) }
    #[inline] pub fn insert<T: Component>(&mut self, component: T) -> &mut Self { self.world.insert(self.entity, component); self }
    #[inline] pub fn remove<T: Component>(&mut self) -> Option<T> { self.world.remove(self.entity) }
    #[inline] pub fn despawn(self) { self.world.despawn(self.entity); }
}

impl fmt::Debug for EntityMut<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        EntityRef { world: self.world, entity: self.entity }.fmt(f)
    }
}
