use std::time::Duration;

use anyhow::Result;
use sea_orm::sea_query::Expr;
use sea_orm::{
    ConnectOptions, Database, DatabaseConnection, EntityOrSelect, EntityTrait, ModelTrait,
    PaginatorTrait, QueryFilter, QueryOrder, QuerySelect, TransactionTrait,
};
use time::OffsetDateTime;
use tracing::error;
use warpgate_common::helpers::fs::secure_file;
use warpgate_common::{GlobalParams, WarpgateConfig, WarpgateError};
use warpgate_db_migrations::migrate_database;

use crate::recordings::SessionRecordings;

const LOG_PRUNE_PAGE_SIZE: u64 = 256;
const DEFAULT_LOG_MAX_SIZE_MEGABYTES: i64 = 512;
const MIN_LOG_MAX_SIZE_MEGABYTES: i64 = 1;
const BYTES_PER_MEGABYTE: u64 = 1_048_576;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LogRetentionStrategy {
    MaxAge,
    MaxSize,
}

impl LogRetentionStrategy {
    fn parse(value: &str) -> Self {
        if value == "max_size" {
            Self::MaxSize
        } else {
            Self::MaxAge
        }
    }
}

fn clamp_log_max_size_megabytes(size_mb: Option<i64>) -> i64 {
    size_mb
        .unwrap_or(DEFAULT_LOG_MAX_SIZE_MEGABYTES)
        .max(MIN_LOG_MAX_SIZE_MEGABYTES)
}

fn log_size_limit_bytes(size_mb: i64) -> Result<u64> {
    let size_mb = u64::try_from(size_mb)?;
    Ok(size_mb.saturating_mul(BYTES_PER_MEGABYTE))
}

fn estimate_log_entry_size(entry: &warpgate_db_entities::LogEntry::Model) -> u64 {
    let mut size = u64::try_from(entry.text.len()).unwrap_or(u64::MAX)
        + u64::try_from(entry.target.len()).unwrap_or(u64::MAX);

    if let Some(username) = &entry.username {
        size = size.saturating_add(u64::try_from(username.len()).unwrap_or(u64::MAX));
    }
    if let Some(related_users) = &entry.related_users {
        size = size.saturating_add(u64::try_from(related_users.len()).unwrap_or(u64::MAX));
    }
    if let Some(related_access_roles) = &entry.related_access_roles {
        size = size.saturating_add(u64::try_from(related_access_roles.len()).unwrap_or(u64::MAX));
    }
    if let Some(related_admin_roles) = &entry.related_admin_roles {
        size = size.saturating_add(u64::try_from(related_admin_roles.len()).unwrap_or(u64::MAX));
    }

    // Approximate JSON payload and fixed-size scalar fields.
    size.saturating_add(u64::try_from(entry.values.to_string().len()).unwrap_or(u64::MAX))
        .saturating_add(16) // session_id
        .saturating_add(16) // id
        .saturating_add(16) // timestamp rough accounting
}

async fn trim_non_audit_logs_by_size(
    db: &DatabaseConnection,
    max_size_megabytes: i64,
) -> Result<(), WarpgateError> {
    use warpgate_db_entities::LogEntry;

    let max_size_bytes = log_size_limit_bytes(max_size_megabytes).map_err(WarpgateError::from)?;
    let mut total_size: u64 = 0;
    let mut to_delete = Vec::new();

    let mut pages = LogEntry::Entity::find()
        .filter(Expr::col(LogEntry::Column::Target).ne("audit"))
        .order_by_asc(LogEntry::Column::Timestamp)
        .paginate(db, LOG_PRUNE_PAGE_SIZE);

    while let Some(items) = pages.fetch_and_next().await? {
        for item in items {
            total_size = total_size.saturating_add(estimate_log_entry_size(&item));
            to_delete.push(item.id);
        }
    }

    if total_size <= max_size_bytes {
        return Ok(());
    }

    let mut kept_size = total_size;
    let mut delete_count = 0usize;
    let mut pages = LogEntry::Entity::find()
        .filter(Expr::col(LogEntry::Column::Target).ne("audit"))
        .order_by_asc(LogEntry::Column::Timestamp)
        .paginate(db, LOG_PRUNE_PAGE_SIZE);

    while kept_size > max_size_bytes {
        let Some(items) = pages.fetch_and_next().await? else {
            break;
        };

        for item in items {
            if kept_size <= max_size_bytes {
                break;
            }
            kept_size = kept_size.saturating_sub(estimate_log_entry_size(&item));
            delete_count = delete_count.saturating_add(1);
        }
    }

    if delete_count == 0 {
        return Ok(());
    }

    let ids = to_delete.into_iter().take(delete_count).collect::<Vec<_>>();
    if ids.is_empty() {
        return Ok(());
    }

    LogEntry::Entity::delete_many()
        .filter(Expr::col(LogEntry::Column::Id).is_in(ids))
        .exec(db)
        .await?;

    Ok(())
}

pub async fn connect_to_db(
    config: &WarpgateConfig,
    params: &GlobalParams,
) -> Result<DatabaseConnection> {
    let mut url = url::Url::parse(&config.store.database_url.expose_secret()[..])?;
    if url.scheme() == "sqlite" {
        let path = url.path();
        let mut abs_path = params.paths_relative_to().clone();
        abs_path.push(path);
        abs_path.push("db.sqlite3");

        if let Some(parent) = abs_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        url.set_path(
            abs_path
                .to_str()
                .ok_or_else(|| anyhow::anyhow!("Failed to convert database path to string"))?,
        );

        url.set_query(Some("mode=rwc"));

        let db = Database::connect(ConnectOptions::new(url.to_string())).await?;
        db.begin().await?.commit().await?;

        if params.should_secure_files() {
            secure_file(&abs_path)?;
        }
    }

    let mut opt = ConnectOptions::new(url.to_string());
    opt.max_connections(100)
        .min_connections(5)
        .connect_timeout(Duration::from_secs(8))
        .idle_timeout(Duration::from_secs(8))
        .max_lifetime(Duration::from_secs(8))
        .sqlx_logging(true);

    let connection = Database::connect(opt).await?;

    migrate_database(&connection).await?;
    Ok(connection)
}

pub async fn populate_db(
    db: &DatabaseConnection,
    _config: &mut WarpgateConfig,
) -> Result<(), WarpgateError> {
    use sea_orm::ActiveValue::Set;
    use warpgate_db_entities::{Recording, Session};

    Recording::Entity::update_many()
        .set(Recording::ActiveModel {
            ended: Set(Some(OffsetDateTime::now_utc())),
            ..Default::default()
        })
        .filter(Expr::col(Recording::Column::Ended).is_null())
        .exec(db)
        .await
        .map_err(WarpgateError::from)?;

    Session::Entity::update_many()
        .set(Session::ActiveModel {
            ended: Set(Some(OffsetDateTime::now_utc())),
            ..Default::default()
        })
        .filter(Expr::col(Session::Column::Ended).is_null())
        .exec(db)
        .await
        .map_err(WarpgateError::from)?;

    Ok(())
}

pub async fn cleanup_db(
    db: &DatabaseConnection,
    recordings: &SessionRecordings,
    retention: &Duration,
    audit_retention: &Duration,
) -> Result<()> {
    use warpgate_db_entities::{LogEntry, Parameters, Recording, Session, Ticket, TicketRequest};
    let audit_cutoff = OffsetDateTime::now_utc() - time::Duration::try_from(*audit_retention)?;
    let recording_cutoff = OffsetDateTime::now_utc() - time::Duration::try_from(*retention)?;
    let parameters = Parameters::Entity::get(db).await?;
    let log_strategy = LogRetentionStrategy::parse(&parameters.log_retention_strategy);
    let log_size_limit_mb = clamp_log_max_size_megabytes(parameters.log_max_size_megabytes);

    LogEntry::Entity::delete_many()
        .filter(Expr::col(LogEntry::Column::Target).eq("audit"))
        .filter(Expr::col(LogEntry::Column::Timestamp).lt(audit_cutoff))
        .exec(db)
        .await?;

    match log_strategy {
        LogRetentionStrategy::MaxAge => {
            LogEntry::Entity::delete_many()
                .filter(Expr::col(LogEntry::Column::Target).ne("audit"))
                .filter(Expr::col(LogEntry::Column::Timestamp).lt(recording_cutoff))
                .exec(db)
                .await?;
        }
        LogRetentionStrategy::MaxSize => {
            trim_non_audit_logs_by_size(db, log_size_limit_mb).await?;
        }
    }

    {
        let active_ticket_ids = Ticket::Entity::find()
            .select()
            .column(Ticket::Column::Id)
            .filter(
                Expr::col(Ticket::Column::Expiry)
                    .is_null()
                    .or(Expr::col(Ticket::Column::Expiry).gt(OffsetDateTime::now_utc())),
            )
            .all(db)
            .await?
            .into_iter()
            .map(|x| x.id)
            .collect::<Vec<_>>();

        let mut request_deletion = TicketRequest::Entity::delete_many()
            .filter(Expr::col(TicketRequest::Column::Created).lt(audit_cutoff));

        if !active_ticket_ids.is_empty() {
            request_deletion = request_deletion.filter(
                Expr::col(TicketRequest::Column::TicketId)
                    .is_null()
                    .or(Expr::col(TicketRequest::Column::TicketId).is_not_in(active_ticket_ids)),
            );
        }

        request_deletion.exec(db).await?;
    }

    let recordings_to_delete = Recording::Entity::find()
        .filter(Expr::col(Session::Column::Ended).is_not_null())
        .filter(Expr::col(Session::Column::Ended).lt(recording_cutoff))
        .all(db)
        .await?;

    for recording in recordings_to_delete {
        if let Err(error) = recordings
            .remove(&recording.session_id, &recording.name)
            .await
        {
            error!(session=%recording.session_id, name=%recording.name, %error, "Failed to remove recording");
        }
        recording.delete(db).await?;
    }

    Session::Entity::delete_many()
        .filter(Expr::col(Session::Column::Ended).is_not_null())
        .filter(Expr::col(Session::Column::Ended).lt(recording_cutoff))
        .exec(db)
        .await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use sea_orm::ActiveValue::Set;
    use sea_orm::{ActiveModelTrait, ConnectionTrait, Database, EntityTrait, Schema};
    use serde_json::json;
    use uuid::Uuid;
    use warpgate_db_entities::LogEntry;

    use super::*;

    #[test]
    fn parses_log_retention_strategy() {
        assert_eq!(
            LogRetentionStrategy::parse("max_age"),
            LogRetentionStrategy::MaxAge
        );
        assert_eq!(
            LogRetentionStrategy::parse("max_size"),
            LogRetentionStrategy::MaxSize
        );
        // Unknown values must fail safe to max_age.
        assert_eq!(
            LogRetentionStrategy::parse("unexpected"),
            LogRetentionStrategy::MaxAge
        );
    }

    #[test]
    fn clamps_size_limit_to_positive_value() {
        assert_eq!(
            clamp_log_max_size_megabytes(None),
            DEFAULT_LOG_MAX_SIZE_MEGABYTES
        );
        assert_eq!(
            clamp_log_max_size_megabytes(Some(0)),
            MIN_LOG_MAX_SIZE_MEGABYTES
        );
        assert_eq!(
            clamp_log_max_size_megabytes(Some(-10)),
            MIN_LOG_MAX_SIZE_MEGABYTES
        );
        assert_eq!(clamp_log_max_size_megabytes(Some(2048)), 2048);
    }

    #[test]
    fn converts_size_limit_to_bytes() -> Result<()> {
        assert_eq!(log_size_limit_bytes(1)?, BYTES_PER_MEGABYTE);
        assert_eq!(log_size_limit_bytes(10)?, BYTES_PER_MEGABYTE * 10);
        Ok(())
    }

    #[tokio::test]
    async fn trims_oldest_non_audit_logs_when_size_limit_exceeded() -> Result<()> {
        let db = Database::connect("sqlite::memory:").await?;
        let builder = db.get_database_backend();
        let schema = Schema::new(builder);
        db.execute(builder.build(&schema.create_table_from_entity(LogEntry::Entity)))
            .await?;

        let session_id = Uuid::new_v4();
        let base = OffsetDateTime::now_utc();
        let large_text = "x".repeat(450_000);

        for (offset_seconds, target) in [(0_i64, "ssh"), (1, "ssh"), (2, "ssh"), (3, "audit")] {
            LogEntry::ActiveModel {
                id: Set(Uuid::new_v4()),
                text: Set(large_text.clone()),
                target: Set(target.to_owned()),
                values: Set(json!({})),
                timestamp: Set(base + time::Duration::seconds(offset_seconds)),
                session_id: Set(session_id),
                username: Set(None),
                related_users: Set(None),
                related_access_roles: Set(None),
                related_admin_roles: Set(None),
            }
            .insert(&db)
            .await?;
        }

        trim_non_audit_logs_by_size(&db, 1).await?;

        let remaining = LogEntry::Entity::find()
            .order_by_asc(LogEntry::Column::Timestamp)
            .all(&db)
            .await?;
        let non_audit_remaining = remaining
            .iter()
            .filter(|entry| entry.target != "audit")
            .collect::<Vec<_>>();
        assert_eq!(non_audit_remaining.len(), 2);
        assert_eq!(
            non_audit_remaining[0].timestamp,
            base + time::Duration::seconds(1)
        );
        assert_eq!(
            non_audit_remaining[1].timestamp,
            base + time::Duration::seconds(2)
        );
        assert!(remaining.iter().any(|entry| entry.target == "audit"));

        Ok(())
    }
}
