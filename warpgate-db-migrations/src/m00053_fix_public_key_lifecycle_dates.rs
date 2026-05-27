use sea_orm::DbBackend;
use sea_orm_migration::prelude::*;

use crate::m00009_credential_models::public_key_credential;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m00053_fix_public_key_lifecycle_dates"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let connection = manager.get_connection();
        if connection.get_database_backend() != DbBackend::Sqlite {
            manager
                .alter_table(
                    Table::alter()
                        .table(public_key_credential::Entity)
                        .modify_column(
                            ColumnDef::new(Alias::new("expires_at"))
                                .date_time()
                                .timestamp_with_time_zone()
                                .null(),
                        )
                        .to_owned(),
                )
                .await?;

            manager
                .alter_table(
                    Table::alter()
                        .table(public_key_credential::Entity)
                        .modify_column(
                            ColumnDef::new(Alias::new("revoked_at"))
                                .date_time()
                                .timestamp_with_time_zone()
                                .null(),
                        )
                        .to_owned(),
                )
                .await?;
        }

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let connection = manager.get_connection();
        if connection.get_database_backend() != DbBackend::Sqlite {
            manager
                .alter_table(
                    Table::alter()
                        .table(public_key_credential::Entity)
                        .modify_column(ColumnDef::new(Alias::new("expires_at")).date_time().null())
                        .to_owned(),
                )
                .await?;

            manager
                .alter_table(
                    Table::alter()
                        .table(public_key_credential::Entity)
                        .modify_column(ColumnDef::new(Alias::new("revoked_at")).date_time().null())
                        .to_owned(),
                )
                .await?;
        }

        Ok(())
    }
}
