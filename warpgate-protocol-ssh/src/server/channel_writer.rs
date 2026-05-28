use anyhow::Result;
use bytes::Bytes;
use russh::ChannelId;
use russh::server::Handle;
use tokio::sync::mpsc;

const CHANNEL_WRITE_QUEUE_CAPACITY: usize = 1024;

#[derive(Debug)]
enum ChannelWriteOperation {
    Data(Handle, ChannelId, Bytes),
    ExtendedData(Handle, ChannelId, u32, Bytes),
    Flush(tokio::sync::oneshot::Sender<()>),
}

/// Sequences data writes and runs them in background to avoid lockups
pub struct ChannelWriter {
    tx: mpsc::Sender<ChannelWriteOperation>,
}

impl ChannelWriter {
    pub fn new() -> Self {
        let (tx, mut rx) = mpsc::channel::<ChannelWriteOperation>(CHANNEL_WRITE_QUEUE_CAPACITY);
        tokio::spawn(async move {
            while let Some(operation) = rx.recv().await {
                match operation {
                    ChannelWriteOperation::Data(handle, channel, data) => {
                        let _ = handle.data(channel, data).await;
                    }
                    ChannelWriteOperation::ExtendedData(handle, channel, ext, data) => {
                        let _ = handle.extended_data(channel, ext, data).await;
                    }
                    ChannelWriteOperation::Flush(reply) => {
                        let _ = reply.send(());
                    }
                }
            }
        });
        Self { tx }
    }

    pub async fn write<D: Into<Bytes>>(
        &self,
        handle: Handle,
        channel: ChannelId,
        data: D,
    ) -> Result<()> {
        self.tx
            .send(ChannelWriteOperation::Data(handle, channel, data.into()))
            .await
            .map_err(|_| anyhow::anyhow!("ChannelWriter task has stopped"))
    }

    pub async fn write_extended<D: Into<Bytes>>(
        &self,
        handle: Handle,
        channel: ChannelId,
        ext: u32,
        data: D,
    ) -> Result<()> {
        self.tx
            .send(ChannelWriteOperation::ExtendedData(
                handle,
                channel,
                ext,
                data.into(),
            ))
            .await
            .map_err(|_| anyhow::anyhow!("ChannelWriter task has stopped"))
    }

    /// Flush all pending writes. Returns when all previously queued operations have completed.
    pub async fn flush(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.tx
            .send(ChannelWriteOperation::Flush(tx))
            .await
            .map_err(|_| "ChannelWriter task has stopped")?;
        rx.await.map_err(|_| "ChannelWriter flush failed")?;
        Ok(())
    }
}
