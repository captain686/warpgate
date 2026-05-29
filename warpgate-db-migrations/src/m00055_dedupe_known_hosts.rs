use sea_orm::DbBackend;
use sea_orm_migration::prelude::*;
use sea_orm_migration::sea_orm::ConnectionTrait;

use crate::m00004_create_known_host::known_host;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m00055_dedupe_known_hosts"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        match manager.get_database_backend() {
            DbBackend::MySql => {
                db.execute_unprepared(
                    "DELETE FROM known_hosts WHERE id NOT IN \
                     (SELECT id FROM (SELECT MIN(id) AS id FROM known_hosts \
                     GROUP BY host, port, key_type, key_base64) AS t)",
                )
                .await?;
            }
            DbBackend::Postgres => {
                db.execute_unprepared(
                    "DELETE FROM known_hosts WHERE id::text NOT IN \
                     (SELECT MIN(id::text) FROM known_hosts \
                     GROUP BY host, port, key_type, key_base64)",
                )
                .await?;
            }
            DbBackend::Sqlite => {
                db.execute_unprepared(
                    "DELETE FROM known_hosts WHERE id NOT IN \
                     (SELECT MIN(id) FROM known_hosts \
                     GROUP BY host, port, key_type, key_base64)",
                )
                .await?;
            }
        }

        manager
            .create_index(
                Index::create()
                    .name("known_hosts_host_port_key_unique")
                    .table(known_host::Entity)
                    .col(known_host::Column::Host)
                    .col(known_host::Column::Port)
                    .col(known_host::Column::KeyType)
                    .col(known_host::Column::KeyBase64)
                    .unique()
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_index(
                Index::drop()
                    .name("known_hosts_host_port_key_unique")
                    .table(known_host::Entity)
                    .to_owned(),
            )
            .await
    }
}
