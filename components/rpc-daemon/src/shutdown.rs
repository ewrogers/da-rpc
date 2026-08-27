use tokio::sync::watch;

#[derive(Clone)]
pub(crate) struct Shutdown(watch::Sender<bool>);

impl Shutdown {
    pub(crate) fn new() -> Self {
        let (sender, _) = watch::channel(false);
        Self(sender)
    }

    pub(crate) fn cancel(&self) {
        self.0.send_replace(true);
    }

    pub(crate) fn is_cancelled(&self) -> bool {
        *self.0.borrow()
    }

    pub(crate) fn subscribe(&self) -> watch::Receiver<bool> {
        self.0.subscribe()
    }

    pub(crate) async fn cancelled(self) {
        let mut receiver = self.subscribe();
        if self.is_cancelled() {
            return;
        }
        while receiver.changed().await.is_ok() {
            if self.is_cancelled() {
                return;
            }
        }
    }
}
