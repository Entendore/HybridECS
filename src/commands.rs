use crate::bundle::Bundle;
use crate::component::Component;
use crate::entity::Entity;
use crate::world::World;

#[must_use = "CommandQueue must be applied to take effect"]
pub struct CommandQueue {
    pub(crate) commands: Vec<Box<dyn FnOnce(&mut World) + Send + Sync>>,
}

impl CommandQueue {
    pub fn new() -> Self { Self { commands: Vec::new() } }
    pub fn apply(&mut self, world: &mut World) {
        for cmd in self.commands.drain(..) { cmd(world); }
    }
}

#[must_use = "Commands should be used to queue mutations"]
pub struct Commands<'w> {
    pub(crate) world: &'w mut World,
    pub(crate) queue: &'w mut CommandQueue,
}

impl<'w> Commands<'w> {
    pub fn new(world: &'w mut World, queue: &'w mut CommandQueue) -> Self { Self { world, queue } }
    pub fn spawn_empty(&mut self) -> Entity { self.world.spawn_empty() }
    pub fn spawn<B: Bundle>(&mut self, bundle: B) -> Entity {
        let entity = self.spawn_empty();
        self.queue.commands.push(Box::new(move |world: &mut World| { world.insert_bundle(entity, bundle); }));
        entity
    }
    pub fn entity(&mut self, entity: Entity) -> EntityCommands { EntityCommands { entity, commands: self } }
    pub fn despawn(&mut self, entity: Entity) {
        self.queue.commands.push(Box::new(move |world: &mut World| { world.despawn(entity); }));
    }
}

#[must_use = "EntityCommands should be used to queue mutations"]
pub struct EntityCommands<'a, 'b> {
    pub(crate) entity: Entity,
    pub(crate) commands: &'b mut Commands<'a>,
}

impl<'a, 'b> EntityCommands<'a, 'b> {
    pub fn insert<T: Component>(&mut self, component: T) -> &mut Self {
        let entity = self.entity;
        self.commands.queue.commands.push(Box::new(move |world: &mut World| { world.insert(entity, component); }));
        self
    }
    pub fn remove<T: Component>(&mut self) -> &mut Self {
        let entity = self.entity;
        self.commands.queue.commands.push(Box::new(move |world: &mut World| { world.remove::<T>(entity); }));
        self
    }
    pub fn despawn(&mut self) {
        let entity = self.entity;
        self.commands.despawn(entity);
    }
}
