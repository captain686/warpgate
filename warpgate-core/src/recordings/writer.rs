use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use bytes::Bytes;
use sea_orm::{ActiveModelTrait, DatabaseConnection, EntityTrait};
use time::OffsetDateTime;
use tokio::fs::File;
use tokio::io::{AsyncWriteExt, BufWriter};
use tokio::sync::{Mutex, broadcast, mpsc, oneshot};
use tracing::error;
use uuid::Uuid;
use warpgate_common::helpers::fs::secure_file;
use warpgate_common::{GlobalParams, try_block};
use warpgate_db_entities::Recording;

use super::{Error, Result};

enum RecordingWriterCommand {
    Write(Bytes),
    Flush(oneshot::Sender<()>),
    Finish(oneshot::Sender<()>),
}

#[derive(Clone)]
pub struct RecordingWriter {
    sender: mpsc::Sender<RecordingWriterCommand>,
    live_sender: broadcast::Sender<Bytes>,
    drop_signal: mpsc::Sender<()>,
}

impl RecordingWriter {
    pub(crate) async fn new(
        path: PathBuf,
        model: Recording::Model,
        db: Arc<Mutex<DatabaseConnection>>,
        live: Arc<Mutex<HashMap<Uuid, broadcast::Sender<Bytes>>>>,
        params: &GlobalParams,
    ) -> Result<Self> {
        let file = File::options()
            .append(true)
            .create(true)
            .open(&path)
            .await?;
        if params.should_secure_files() {
            secure_file(&path)?;
        }
        let mut writer = BufWriter::new(file);
        let (sender, mut receiver) = mpsc::channel::<RecordingWriterCommand>(1024);
        let (drop_signal, mut drop_receiver) = mpsc::channel(1);

        let live_sender = broadcast::channel(128).0;
        {
            let mut live = live.lock().await;
            live.insert(model.id, live_sender.clone());
        }

        tokio::spawn({
            let live = live.clone();
            let id = model.id;
            async move {
                let _ = drop_receiver.recv().await;
                let mut live = live.lock().await;
                live.remove(&id);
            }
        });

        tokio::spawn(async move {
            let mut finish_reply = None;
            try_block!(async {
                let mut last_flush = Instant::now();
                loop {
                    if last_flush.elapsed() > Duration::from_secs(5) {
                        last_flush = Instant::now();
                        writer.flush().await?;
                    }
                    tokio::select! {
                        command = receiver.recv() => match command {
                            Some(RecordingWriterCommand::Write(bytes)) => {
                                writer.write_all(&bytes).await?;
                            }
                            Some(RecordingWriterCommand::Flush(reply)) => {
                                writer.flush().await?;
                                last_flush = Instant::now();
                                let _ = reply.send(());
                            }
                            Some(RecordingWriterCommand::Finish(reply)) => {
                                writer.flush().await?;
                                finish_reply = Some(reply);
                                break;
                            }
                            None => break,
                        },
                        () = tokio::time::sleep(Duration::from_millis(5000)) => ()
                    }
                }
                Ok::<(), anyhow::Error>(())
            } catch (error: anyhow::Error) {
                error!(%error, ?path, "Failed to write recording");
            });

            try_block!(async {
                use sea_orm::ActiveValue::Set;

                writer.flush().await?;

                let id = model.id;
                let db = db.lock().await;
                let recording = Recording::Entity::find_by_id(id)
                    .one(&*db)
                    .await?
                    .ok_or_else(|| anyhow::anyhow!("Recording not found"))?;
                let mut model: Recording::ActiveModel = recording.into();
                model.ended = Set(Some(OffsetDateTime::now_utc()));
                model.update(&*db).await?;
                Ok::<(), anyhow::Error>(())
            } catch (error: anyhow::Error) {
                error!(%error, ?path, "Failed to write recording");
            });

            if let Some(reply) = finish_reply {
                let _ = reply.send(());
            }
        });

        Ok(Self {
            sender,
            live_sender,
            drop_signal,
        })
    }

    pub async fn write(&self, data: &[u8]) -> Result<()> {
        let data = Bytes::from(data.to_vec());
        self.sender
            .send(RecordingWriterCommand::Write(data.clone()))
            .await
            .map_err(|_| Error::Closed)?;
        let _ = self.live_sender.send(data);
        Ok(())
    }

    pub async fn flush(&self) -> Result<()> {
        let (tx, rx) = oneshot::channel();
        self.sender
            .send(RecordingWriterCommand::Flush(tx))
            .await
            .map_err(|_| Error::Closed)?;
        rx.await.map_err(|_| Error::Closed)
    }

    pub async fn finish(self) -> Result<()> {
        let (tx, rx) = oneshot::channel();
        self.sender
            .send(RecordingWriterCommand::Finish(tx))
            .await
            .map_err(|_| Error::Closed)?;
        rx.await.map_err(|_| Error::Closed)
    }
}

impl Drop for RecordingWriter {
    fn drop(&mut self) {
        let signal = std::mem::replace(&mut self.drop_signal, mpsc::channel(1).0);
        tokio::spawn(async move { signal.send(()).await });
    }
}
