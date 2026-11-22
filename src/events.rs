use crate::component::Mut;
use crate::resource::Resource;

pub trait Event: Send + Sync + 'static {}
impl<T: Send + Sync + 'static> Event for T {}

#[derive(Event, Debug, Clone)]
pub struct ErrorEvent {
    pub system_name: String,
    pub message: String,
}

#[derive(Default)]
pub struct Events<T: Event> {
    current: Vec<T>,
    previous: Vec<T>,
}

impl<T: Event> Resource for Events<T> {}

impl<T: Event> Events<T> {
    pub fn flush(&mut self) {
        std::mem::swap(&mut self.current, &mut self.previous);
        self.current.clear();
    }
}

pub struct EventWriter<'a, T: Event> { pub(crate) events: Mut<'a, Events<T>> }
impl<'a, T: Event> EventWriter<'a, T> {
    pub fn send(&mut self, event: T) { self.events.current.push(event); }
}

pub struct EventReader<'a, T: Event> { pub(crate) events: &'a Events<T> }
impl<'a, T: Event> EventReader<'a, T> {
    pub fn iter(&self) -> impl Iterator<Item = &T> { self.events.previous.iter() }
}
