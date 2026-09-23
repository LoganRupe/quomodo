use std::sync::OnceLock;

use komodo_client::entities::update::UpdateListItem;
use tokio::sync::{Mutex, broadcast};

pub fn update_channel() -> &'static BroadcastChannel<UpdateListItem> {
  static UPDATE_CHANNEL: OnceLock<BroadcastChannel<UpdateListItem>> =
    OnceLock::new();
  UPDATE_CHANNEL.get_or_init(|| BroadcastChannel::new(100))
}

pub struct BroadcastChannel<T> {
  pub sender: Mutex<broadcast::Sender<T>>,
  pub receiver: broadcast::Receiver<T>,
}

impl<T: Clone> BroadcastChannel<T> {
  pub fn new(capacity: usize) -> BroadcastChannel<T> {
    let (sender, receiver) = broadcast::channel(capacity);
    BroadcastChannel {
      sender: sender.into(),
      receiver,
    }
  }
}
