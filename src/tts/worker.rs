use async_trait::async_trait;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::errors::{NCBError, Result};

#[async_trait]
pub(crate) trait Processor<T>: Send + 'static {
    async fn process(&mut self, item: T) -> Result<()>;
}

/// One bounded FIFO per session; controls bypass the work queue.
pub(crate) struct WorkQueue<T> {
    sender: mpsc::Sender<T>,
    stopped: CancellationToken,
    current: Arc<Mutex<Option<CancellationToken>>>,
}

impl<T: Send + 'static> WorkQueue<T> {
    pub fn start(capacity: usize, mut processor: impl Processor<T>) -> Self {
        let (sender, mut receiver) = mpsc::channel(capacity);
        let stopped = CancellationToken::new();
        let current = Arc::new(Mutex::new(None));
        let stop_worker = stopped.clone();
        let current_worker = current.clone();
        tokio::spawn(async move {
            loop {
                let item = tokio::select! {
                    biased;
                    _ = stop_worker.cancelled() => break,
                    item = receiver.recv() => match item { Some(item) => item, None => break },
                };
                let cancel = stop_worker.child_token();
                *current_worker.lock().unwrap() = Some(cancel.clone());
                tokio::select! {
                    biased;
                    _ = cancel.cancelled() => {},
                    result = processor.process(item) => {
                        if let Err(error) = result { tracing::warn!(error = %error, "Speech job failed"); }
                    }
                }
                current_worker.lock().unwrap().take();
            }
        });
        Self {
            sender,
            stopped,
            current,
        }
    }

    pub fn enqueue(&self, item: T) -> Result<()> {
        if self.is_stopped() {
            return Err(NCBError::SessionStopped);
        }
        self.sender.try_send(item).map_err(|error| match error {
            mpsc::error::TrySendError::Full(_) => NCBError::QueueFull,
            mpsc::error::TrySendError::Closed(_) => NCBError::SessionStopped,
        })
    }

    pub fn skip(&self) {
        if let Some(current) = self.current.lock().unwrap().as_ref() {
            current.cancel();
        }
    }

    pub fn stop(&self) {
        self.stopped.cancel();
    }
    pub fn is_stopped(&self) -> bool {
        self.stopped.is_cancelled()
    }
    pub fn cancellation(&self) -> CancellationToken {
        self.stopped.clone()
    }
}

impl<T> Drop for WorkQueue<T> {
    fn drop(&mut self) {
        self.stopped.cancel();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use tokio::sync::Notify;

    struct TestProcessor {
        entered: mpsc::UnboundedSender<usize>,
        completed: mpsc::UnboundedSender<usize>,
        gate: Arc<Notify>,
    }
    #[async_trait]
    impl Processor<usize> for TestProcessor {
        async fn process(&mut self, item: usize) -> Result<()> {
            self.entered.send(item).unwrap();
            if item == 1 {
                self.gate.notified().await;
            }
            self.completed.send(item).unwrap();
            Ok(())
        }
    }

    #[tokio::test]
    async fn isolated_fifo_with_bounded_queue_and_interruptible_synthesis() {
        let (entered, mut entries) = mpsc::unbounded_channel();
        let (completed, mut completions) = mpsc::unbounded_channel();
        let gate = Arc::new(Notify::new());
        let queue = WorkQueue::start(
            1,
            TestProcessor {
                entered: entered.clone(),
                completed: completed.clone(),
                gate: gate.clone(),
            },
        );
        queue.enqueue(1).unwrap();
        assert_eq!(entries.recv().await, Some(1));
        queue.enqueue(2).unwrap();
        assert!(matches!(queue.enqueue(3), Err(NCBError::QueueFull)));
        let other = WorkQueue::start(
            1,
            TestProcessor {
                entered,
                completed,
                gate: gate.clone(),
            },
        );
        other.enqueue(4).unwrap();
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(1), completions.recv())
                .await
                .unwrap(),
            Some(4)
        );
        queue.skip();
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(1), completions.recv())
                .await
                .unwrap(),
            Some(2)
        );
        gate.notify_one();
        assert!(completions.try_recv().is_err());
        queue.stop();
        assert!(matches!(queue.enqueue(5), Err(NCBError::SessionStopped)));
        other.stop();
    }

    #[tokio::test]
    async fn stop_discards_current_and_pending_work() {
        let (entered, mut entries) = mpsc::unbounded_channel();
        let (completed, mut completions) = mpsc::unbounded_channel();
        let gate = Arc::new(Notify::new());
        let queue = WorkQueue::start(
            2,
            TestProcessor {
                entered,
                completed,
                gate: gate.clone(),
            },
        );
        queue.enqueue(1).unwrap();
        entries.recv().await.unwrap();
        queue.enqueue(2).unwrap();
        queue.stop();
        gate.notify_one();
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(1), completions.recv())
                .await
                .unwrap(),
            None
        );
    }
}
