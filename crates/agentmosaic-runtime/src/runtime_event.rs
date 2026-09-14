//! Runtime-observation delivery without granting observations task authority.

use std::path::PathBuf;
use std::sync::Arc;

use agentmosaic_storage::SqliteTaskBoard;
use agentmosaic_team::{RuntimeEventPolicy, RuntimeEventRecord};

/// Best-effort presentation sink. Implementations cannot return an execution
/// error, and the dispatcher contains a panicking observer as well.
pub trait LiveRuntimeEventSink: Send + Sync {
    fn emit(&self, record: &RuntimeEventRecord);
}

#[derive(Debug, Default)]
pub struct NoopLiveRuntimeEventSink;

impl LiveRuntimeEventSink for NoopLiveRuntimeEventSink {
    fn emit(&self, _record: &RuntimeEventRecord) {}
}

/// Required durable observation boundary. A write error is visible to the
/// runtime adapter so it cannot report success without its required audit
/// boundary.
pub trait DurableRuntimeEventWriter: Send + Sync {
    fn write(&self, record: RuntimeEventRecord) -> Result<(), String>;
}

#[derive(Debug, Clone)]
pub struct SqliteRuntimeEventWriter {
    database: PathBuf,
}

impl SqliteRuntimeEventWriter {
    pub fn new(database: PathBuf) -> Result<Self, String> {
        if database.as_os_str().is_empty() {
            return Err("runtime event database path is required".into());
        }
        Ok(Self { database })
    }
}

impl DurableRuntimeEventWriter for SqliteRuntimeEventWriter {
    fn write(&self, record: RuntimeEventRecord) -> Result<(), String> {
        let connection = rusqlite::Connection::open(&self.database).map_err(|e| e.to_string())?;
        let mut board = SqliteTaskBoard::open(connection).map_err(|e| e.to_string())?;
        board
            .append_runtime_event(record)
            .map(|_| ())
            .map_err(|e| e.to_string())
    }
}

/// One facade used by adapters. Live delivery is always attempted and cannot
/// fail execution. Durable events additionally cross the fallible writer
/// boundary before an adapter may report success.
#[derive(Clone)]
pub struct RuntimeEventDispatcher {
    live: Arc<dyn LiveRuntimeEventSink>,
    durable: Arc<dyn DurableRuntimeEventWriter>,
}

impl RuntimeEventDispatcher {
    pub fn new(
        live: Arc<dyn LiveRuntimeEventSink>,
        durable: Arc<dyn DurableRuntimeEventWriter>,
    ) -> Self {
        Self { live, durable }
    }

    pub fn emit(&self, record: RuntimeEventRecord) -> Result<(), String> {
        let record = record.bounded();
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            self.live.emit(&record);
        }));
        if record.event.policy() == RuntimeEventPolicy::Durable {
            self.durable.write(record)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use agentmosaic_team::RuntimeEvent;

    use super::*;

    struct PanickingLive;

    impl LiveRuntimeEventSink for PanickingLive {
        fn emit(&self, _record: &RuntimeEventRecord) {
            panic!("presentation failed")
        }
    }

    struct CountingWriter {
        writes: AtomicUsize,
        fail: bool,
    }

    impl DurableRuntimeEventWriter for CountingWriter {
        fn write(&self, _record: RuntimeEventRecord) -> Result<(), String> {
            self.writes.fetch_add(1, Ordering::SeqCst);
            if self.fail {
                Err("durable writer failed".into())
            } else {
                Ok(())
            }
        }
    }

    fn record(event: RuntimeEvent) -> RuntimeEventRecord {
        RuntimeEventRecord {
            task_id: 1,
            attempt: 1,
            agent_id: "worker".into(),
            runtime_name: Some("fake".into()),
            native_session_id: Some("session".into()),
            event,
        }
    }

    #[test]
    fn live_observer_panic_is_isolated_from_execution() {
        let writer = Arc::new(CountingWriter {
            writes: AtomicUsize::new(0),
            fail: false,
        });
        let dispatcher = RuntimeEventDispatcher::new(Arc::new(PanickingLive), writer.clone());
        dispatcher
            .emit(record(RuntimeEvent::RuntimeWarning {
                code: None,
                message: "warning".into(),
            }))
            .unwrap();
        assert_eq!(writer.writes.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn durable_failure_fails_closed_but_live_only_does_not_write() {
        let writer = Arc::new(CountingWriter {
            writes: AtomicUsize::new(0),
            fail: true,
        });
        let dispatcher =
            RuntimeEventDispatcher::new(Arc::new(NoopLiveRuntimeEventSink), writer.clone());
        dispatcher
            .emit(record(RuntimeEvent::AssistantMessageDelta {
                text: "partial".into(),
            }))
            .unwrap();
        assert_eq!(writer.writes.load(Ordering::SeqCst), 0);
        assert!(dispatcher
            .emit(record(RuntimeEvent::RuntimeError {
                code: None,
                message: "failed".into(),
            }))
            .unwrap_err()
            .contains("durable writer failed"));
    }
}
