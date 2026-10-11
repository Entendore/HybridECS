use crate::cell::UnsafeWorldCell;
use crate::commands::CommandQueue;
use crate::events::ErrorEvent;
use crate::param::{Access, SystemParamFunction};
use crate::world::World;
use rayon::prelude::*;
use std::any::TypeId;
use std::collections::{HashMap, VecDeque};

pub trait System: Send + Sync {
    type In;
    type Out;
    fn name(&self) -> &'static str { "Unnamed System" }
    fn is_exclusive(&self) -> bool { false }
    fn access(&self) -> Access { Access::default() }
    fn has_commands(&self) -> bool { false }
    unsafe fn run_unsafe(&mut self, input: Self::In, world: UnsafeWorldCell<'_>, queue: &mut CommandQueue) -> Self::Out;
    fn run_exclusive(&mut self, input: Self::In, world: &mut World) -> Self::Out {
        let cell = unsafe { UnsafeWorldCell::new(world) };
        let mut queue = CommandQueue::new();
        let out = unsafe { self.run_unsafe(input, cell, &mut queue) };
        queue.apply(world);
        out
    }
}

pub struct In<T>(pub T);
pub struct Out<T>(pub T);

pub struct PipeSystem<A, B> { a: A, b: B }

impl<A, B> System for PipeSystem<A, B>
where A: System<Out = B::In>, B: System {
    type In = A::In;
    type Out = B::Out;
    fn name(&self) -> &'static str { "PipeSystem" }
    fn is_exclusive(&self) -> bool { self.a.is_exclusive() || self.b.is_exclusive() }
    fn access(&self) -> Access {
        let mut access = self.a.access();
        let b_access = self.b.access();
        access.reads.extend(b_access.reads);
        access.writes.extend(b_access.writes);
        access
    }
    unsafe fn run_unsafe(&mut self, input: Self::In, world: UnsafeWorldCell<'_>, queue: &mut CommandQueue) -> Self::Out {
        let a_out = self.a.run_unsafe(input, world, queue);
        self.b.run_unsafe(a_out, world, queue)
    }
}

pub trait IntoPipe<A: System>: Sized {
    fn pipe<B: System<In = A::Out>>(self, b: B) -> PipeSystem<A, B>;
}
impl<A: System> IntoPipe<A> for A {
    fn pipe<B: System<In = A::Out>>(self, b: B) -> PipeSystem<A, B> { PipeSystem { a: self, b } }
}

pub struct FunctionSystem<F, Marker> { func: F, access: Access, _marker: std::marker::PhantomData<Marker> }

impl<F, Marker> FunctionSystem<F, Marker> where F: SystemParamFunction<Marker> {
    pub fn new(func: F) -> Self { Self { func, access: F::access(), _marker: std::marker::PhantomData } }
}

impl<F, Marker> System for FunctionSystem<F, Marker>
where F: SystemParamFunction<Marker> + Send + Sync {
    type In = ();
    type Out = ();
    fn name(&self) -> &'static str { std::any::type_name::<F>() }
    fn access(&self) -> Access { self.access.clone() }
    fn has_commands(&self) -> bool { self.access.has_commands }
    unsafe fn run_unsafe(&mut self, _input: Self::In, world: UnsafeWorldCell<'_>, queue: &mut CommandQueue) -> Self::Out {
        self.func.run(world, queue);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SystemSet(pub(crate) &'static str);

impl SystemSet {
    pub const fn new(name: &'static str) -> Self { Self(name) }
}

struct ScheduleEntry {
    system: Box<dyn System<In = (), Out = ()>>,
    conditions: Vec<Box<dyn Fn(&World) -> bool + Send + Sync>>,
    sets: Vec<&'static str>,
    before: Vec<&'static str>,
    after: Vec<&'static str>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SystemId(pub(crate) usize);

pub struct Schedule {
    entries: Vec<Option<ScheduleEntry>>,
    run_if: Vec<Box<dyn Fn(&World) -> bool + Send + Sync>>,
    set_ordering: HashMap<(SystemSet, SystemSet), ()>,
}

impl Default for Schedule { fn default() -> Self { Self::new() } }

impl Schedule {
    pub fn new() -> Self { Self { entries: Vec::new(), run_if: Vec::new(), set_ordering: HashMap::new() } }

    pub fn add_system<S: System<In = (), Out = ()> + 'static>(&mut self, system: S) -> SystemBuilder {
        let id = SystemId(self.entries.len());
        self.entries.push(Some(ScheduleEntry {
            system: Box::new(system),
            conditions: Vec::new(),
            sets: Vec::new(),
            before: Vec::new(),
            after: Vec::new(),
        }));
        SystemBuilder { schedule: self, id }
    }

    pub fn add_systems<I, S>(&mut self, systems: I) -> &mut Self
    where I: IntoIterator<Item = S>, S: System<In = (), Out = ()> + 'static {
        for system in systems { self.add_system(system); }
        self
    }

    pub fn configure_set(&mut self, set: SystemSet) -> SetBuilder { SetBuilder { schedule: self, set } }

    pub fn run_if<F: Fn(&World) -> bool + Send + Sync + 'static>(&mut self, condition: F) -> &mut Self {
        self.run_if.push(Box::new(condition));
        self
    }

    pub fn run(&mut self, world: &mut World) {
        if !self.run_if.iter().all(|c| c(world)) { return; }
        let active_indices: Vec<usize> = self.entries.iter().enumerate().filter_map(|(i, e)| e.is_some().then_some(i)).collect();
        
        // Simplified topological sort for brevity in script
        let order = (0..active_indices.len()).collect::<Vec<_>>();
        
        let mut generations: Vec<Vec<usize>> = Vec::new();
        for &i in &order {
            let idx = active_indices[i];
            let entry_access = self.entries[idx].as_ref().unwrap().system.access();
            let mut placed = false;
            for gen in generations.iter_mut().rev() {
                let gen_compatible = gen.iter().all(|&g_idx| {
                    let g_access = self.entries[active_indices[g_idx]].as_ref().unwrap().system.access();
                    entry_access.is_compatible(&g_access)
                });
                if gen_compatible { gen.push(i); placed = true; break; }
            }
            if !placed { generations.push(vec![i]); }
        }

        for gen in generations {
            let mut global_queue = CommandQueue::new();
            if gen.len() == 1 {
                let idx = active_indices[gen[0]];
                let entry = self.entries[idx].as_mut().unwrap();
                let should_run = entry.conditions.iter().all(|c| c(world));
                if should_run {
                    let name = entry.system.name();
                    let span = tracing::span!(tracing::Level::INFO, "system", name = name);
                    let _enter = span.enter();
                    let mut cell = unsafe { UnsafeWorldCell::new(world) };
                    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| { entry.system.run_exclusive((), cell.world_mut()); }));
                    if let Err(e) = result { eprintln!("System '{}' panicked: {:?}", name, e); }
                }
            } else {
                let cell = unsafe { UnsafeWorldCell::new(world) };
                gen.par_iter().map(|&i| &mut self.entries[active_indices[i]].as_mut().unwrap()).for_each(|entry| {
                    let should_run = entry.conditions.iter().all(|c| c(cell.world()));
                    if should_run {
                        let name = entry.system.name();
                        let span = tracing::span!(tracing::Level::INFO, "system", name = name);
                        let _enter = span.enter();
                        let mut local_cell = cell;
                        let mut local_queue = CommandQueue::new();
                        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| { entry.system.run_unsafe((), local_cell, &mut local_queue); }));
                        if let Err(e) = result { eprintln!("System '{}' panicked: {:?}", name, e); }
                    }
                });
            }
            global_queue.apply(world);
        }
    }
}

pub struct SystemBuilder<'a> { schedule: &'a mut Schedule, id: SystemId }

impl<'a> SystemBuilder<'a> {
    pub fn run_if<F: Fn(&World) -> bool + Send + Sync + 'static>(self, condition: F) -> Self {
        self.schedule.entries[self.id.0].as_mut().unwrap().conditions.push(Box::new(condition));
        self
    }
    pub fn in_set(self, set: SystemSet) -> Self { self.schedule.entries[self.id.0].as_mut().unwrap().sets.push(set.0); self }
    pub fn before(self, label: &'static str) -> Self { self.schedule.entries[self.id.0].as_mut().unwrap().before.push(label); self }
    pub fn after(self, label: &'static str) -> Self { self.schedule.entries[self.id.0].as_mut().unwrap().after.push(label); self }
    pub fn id(self) -> SystemId { self.id }
}

pub struct SetBuilder<'a> { schedule: &'a mut Schedule, set: SystemSet }

impl<'a> SetBuilder<'a> {
    pub fn before(self, other: SystemSet) -> Self { self.schedule.set_ordering.insert((self.set, other), ()); self }
    pub fn after(self, other: SystemSet) -> Self { self.schedule.set_ordering.insert((other, self.set), ()); self }
}
