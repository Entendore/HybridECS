use crate::events::Event;
use crate::schedule::Schedule;
use crate::state::NextState;
use crate::world::World;
use std::any::TypeId;
use std::collections::HashMap;

#[derive(Debug, Clone, Copy)]
pub struct FixedTime { pub timestep: f64, pub accumulator: f64 }
impl Default for FixedTime { fn default() -> Self { Self { timestep: 1.0 / 60.0, accumulator: 0.0 } } }

pub struct App {
    pub world: World,
    pub schedule: Schedule,
    pub fixed_schedule: Schedule,
    event_flushers: Vec<Box<dyn Fn(&mut World) + Send + Sync>>,
    pub(crate) state_appliers: Vec<Box<dyn Fn(&mut World) + Send + Sync>>,
    state_enter_schedules: HashMap<TypeId, Box<dyn Fn(&mut World) + Send + Sync>>,
    state_exit_schedules: HashMap<TypeId, Box<dyn Fn(&mut World) + Send + Sync>>,
}

impl Default for App { fn default() -> Self { Self::new() } }
impl From<World> for App {
    fn from(world: World) -> Self { Self { world, schedule: Schedule::default(), fixed_schedule: Schedule::default(), event_flushers: Vec::new(), state_appliers: Vec::new(), state_enter_schedules: HashMap::new(), state_exit_schedules: HashMap::new() } }
}

impl App {
    pub fn new() -> Self {
        Self {
            world: World::new(), schedule: Schedule::new(), fixed_schedule: Schedule::new(),
            event_flushers: Vec::new(), state_appliers: Vec::new(),
            state_enter_schedules: HashMap::new(), state_exit_schedules: HashMap::new(),
        }
    }

    pub fn add_plugin<P: Plugin>(&mut self, plugin: P) -> &mut Self { plugin.build(self); self }
    pub fn add_event<T: Event>(&mut self) -> &mut Self {
        self.world.init_events::<T>();
        self.event_flushers.push(Box::new(move |world| { if let Some(mut events) = world.get_resource_mut::<crate::events::Events<T>>() { events.inner.flush(); } }));
        self
    }
    pub fn on_enter<S: 'static, F: Fn(&mut World) + Send + Sync + 'static>(&mut self, f: F) -> &mut Self {
        self.state_enter_schedules.insert(TypeId::of::<S>(), Box::new(f)); self
    }
    pub fn on_exit<S: 'static, F: Fn(&mut World) + Send + Sync + 'static>(&mut self, f: F) -> &mut Self {
        self.state_exit_schedules.insert(TypeId::of::<S>(), Box::new(f)); self
    }

    pub fn run(&mut self) {
        let delta = 0.016;
        { let mut fixed_time = self.world.get_resource_or_default::<FixedTime>(); fixed_time.accumulator += delta; }
        while self.world.get_resource::<FixedTime>().unwrap().accumulator >= self.world.get_resource::<FixedTime>().unwrap().timestep {
            self.world.get_resource_mut::<FixedTime>().unwrap().accumulator -= self.world.get_resource::<FixedTime>().unwrap().timestep;
            self.fixed_schedule.run(&mut self.world);
        }

        self.schedule.run(&mut self.world);
        for applier in &self.state_appliers { applier(&mut self.world); }
        for flusher in &self.event_flushers { flusher(&mut self.world); }
        self.world.increment_tick();
    }
}

pub trait Plugin { fn build(&self, app: &mut App); }
