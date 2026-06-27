//! A thread-safe pool of fixed-size buffers to avoid heap allocations in the hot path.

use flume::{Receiver, Sender};

/// A thread-safe, lock-free pool of fixed-size byte buffers to eliminate
/// heap allocations in the hot path.
#[derive(Debug, Clone)]
pub struct BufferPool {
    tx: Sender<Vec<u8>>,
    rx: Receiver<Vec<u8>>,
    buffer_size: usize,
}

impl BufferPool {
    /// Creates a new buffer pool with the specified capacity and buffer size.
    #[must_use]
    pub fn new(capacity: usize, buffer_size: usize) -> Self {
        let (tx, rx) = flume::bounded(capacity);
        for _ in 0..capacity {
            let _ = tx.send(vec![0u8; buffer_size]);
        }
        Self {
            tx,
            rx,
            buffer_size,
        }
    }

    /// Leases a buffer from the pool asynchronously.
    #[inline]
    pub async fn lease(&self) -> Vec<u8> {
        match self.rx.recv_async().await {
            Ok(buf) => buf,
            Err(_) => vec![0u8; self.buffer_size],
        }
    }

    /// Leases a buffer from the pool synchronously.
    #[inline]
    #[must_use]
    pub fn lease_sync(&self) -> Vec<u8> {
        match self.rx.recv() {
            Ok(buf) => buf,
            Err(_) => vec![0u8; self.buffer_size],
        }
    }

    /// Releases a buffer back to the pool.
    #[inline]
    pub fn release(&self, mut buf: Vec<u8>) {
        if buf.capacity() < self.buffer_size {
            buf = vec![0u8; self.buffer_size];
        } else {
            buf.resize(self.buffer_size, 0);
        }
        let _ = self.tx.send(buf);
    }
}
