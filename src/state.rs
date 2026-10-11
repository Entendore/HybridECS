use crate::app::App;
use crate::world::World;
use std::marker::PhantomData;
use std::ops::Deref;

#[derive(Debug, Clone, Copy)]
pub struct State<S: Send + Sync + 'static>(pub(crate) S);

impl<S: Send + Sync + 'static> Deref for State<S> {
    type Target = S;
    #[inline] fn deref(&self) -> &Self::Target { &self.0 }
}

#[derive(Debug, Clone, Default)]
pub struct NextState<S: Send + Sync + 'static>(pub(crate) Option<S>);

impl<S: Send + Sync + 'static> NextState<S> {
    pub fn set(&mut self, state: S) { self.0 = Some(state); }
    pub fn reset(&mut self) { self.0 = None; }
}

pub fn in_state<S: Send + Sync + Clone + PartialEq + 'static>(state: S) -> impl Fn(&World) -> bool {
    move |world: &World| { world.get_resource::<State<S>>().map(|s| s.0 == state).unwrap_or(false) }
}

pub trait AddState {
    fn add_state<S: Send + Sync + Clone + PartialEq + 'static>(&mut self, initial: S) -> &mut Self;
}

impl AddState for App {
    fn add_state<S: Send + Sync + Clone + PartialEq + 'static>(&mut self, initial: S) -> &mut Self {
        self.world.insert_resource(State(initial.clone()));
        self.world.insert_resource(NextState::<S>(None));
        self.state_appliers.push(Box::new(move |world: &mut World| {
            if let Some(mut next_state) = world.remove_resource::<NextState<S>>() {
                if let Some(new_state) = next_state.0.take() { world.insert_resource(State(new_state)); }
                world.insert_resource(next_state);
            }
        }));
        self
    }
}
