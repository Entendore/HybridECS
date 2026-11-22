#![forbid(unsafe_op_in_unsafe_fn)]
#![warn(clippy::pedantic, clippy::nursery, missing_docs)]

mod archetype;
mod app;
mod bitset;
mod bundle;
mod cell;
mod commands;
mod component;
mod entity;
mod error;
mod events;
mod hierarchy;
mod network;
mod observer;
mod param;
mod query;
mod reflect;
mod resource;
mod schedule;
mod spatial;
mod state;
mod world;

mod private {
    pub trait SealedQuery {}
    pub trait SealedQueryMut {}
    pub trait SealedFilter {}
    pub trait SealedResource {}
}

pub use archetype::{Archetype, ArchetypeId};
pub use app::{App, FixedTime, Plugin};
pub use bitset::BitSet;
pub use bundle::Bundle;
pub use cell::UnsafeWorldCell;
pub use commands::{CommandQueue, Commands, EntityCommands};
pub use component::{Component, ComponentDescriptor, ComponentPtr, Mut, StorageType};
pub use entity::{Entity, Tick};
pub use error::FetchEntityError;
pub use events::{Event, EventReader, EventWriter, Events, ErrorEvent};
pub use hierarchy::{ChildBuilder, Children, Parent};
pub use network::{EntityMap, MapEntities};
pub use observer::Observer;
pub use param::{Access, Local, NonSendMut, Query as ParamQuery, Res as ParamRes, ResMut as ParamResMut, SystemParam, SystemParamFunction};
pub use query::{
    Added, Changed, Or, Query, QueryIter, QueryFilter, QueryMut, QueryMutIter, QueryState, ReadOnlyQuery, With, Without,
};
pub use reflect::{FromReflect, Reflect, TypeRegistry};
pub use resource::{NonSendMut as ResNonSendMut, Ref, Res, ResMut, Resource, ResourceId};
pub use schedule::{FunctionSystem, IntoPipe, PipeSystem, Schedule, SetBuilder, System, SystemBuilder, SystemSet, SystemId};
pub use spatial::{Aabb, PhysicsWorld, SpatialQuery, broad_phase_system};
pub use state::{in_state, AddState, NextState, State};
pub use world::{EntityBuilder, EntityMut, EntityRef, World};

pub use core::num::NonZeroU32;
pub use rayon;
pub use tracing;
