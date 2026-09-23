use crate::entities::{
  action::ActionActionState, procedure::ProcedureActionState,
  repo::RepoActionState, server::ServerActionState,
  stack::StackActionState, sync::ResourceSyncActionState,
};

pub trait Busy {
  fn busy(&self) -> bool;
}

impl Busy for ServerActionState {
  fn busy(&self) -> bool {
    self.pruning_images || self.pruning_system
  }
}

impl Busy for StackActionState {
  fn busy(&self) -> bool {
    self.pulling
      || self.deploying
      || self.starting
      || self.restarting
      || self.stopping
      || self.destroying
  }
}

impl Busy for RepoActionState {
  fn busy(&self) -> bool {
    self.cloning || self.pulling
  }
}

impl Busy for ProcedureActionState {
  fn busy(&self) -> bool {
    self.running
  }
}

impl Busy for ActionActionState {
  fn busy(&self) -> bool {
    self.running > 0
  }
}

impl Busy for ResourceSyncActionState {
  fn busy(&self) -> bool {
    self.syncing
  }
}
