use sea_orm_migration::prelude::*;

use crate::m00009_credential_models::public_key_credential;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m00048_public_key_credential_lifecycle"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(public_key_credential::Entity)
                    .add_column(
                        ColumnDef::new(Alias::new("issued_by_warpgate"))
                            .boolean()
                            .not_null()
                            .default(false),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .alter_table(
                Table::alter()
                    .table(public_key_credential::Entity)
                    .add_column(ColumnDef::new(Alias::new("expires_at")).date_time().null())
                    .to_owned(),
            )
            .await?;

        manager
            .alter_table(
                Table::alter()
                    .table(public_key_credential::Entity)
                    .add_column(ColumnDef::new(Alias::new("max_uses")).big_integer().null())
                    .to_owned(),
            )
            .await?;

        manager
            .alter_table(
                Table::alter()
                    .table(public_key_credential::Entity)
                    .add_column(ColumnDef::new(Alias::new("uses_left")).big_integer().null())
                    .to_owned(),
            )
            .await?;

        manager
            .alter_table(
                Table::alter()
                    .table(public_key_credential::Entity)
                    .add_column(ColumnDef::new(Alias::new("revoked_at")).date_time().null())
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(public_key_credential::Entity)
                    .drop_column(Alias::new("revoked_at"))
                    .to_owned(),
            )
            .await?;

        manager
            .alter_table(
                Table::alter()
                    .table(public_key_credential::Entity)
                    .drop_column(Alias::new("uses_left"))
                    .to_owned(),
            )
            .await?;

        manager
            .alter_table(
                Table::alter()
                    .table(public_key_credential::Entity)
                    .drop_column(Alias::new("max_uses"))
                    .to_owned(),
            )
            .await?;

        manager
            .alter_table(
                Table::alter()
                    .table(public_key_credential::Entity)
                    .drop_column(Alias::new("expires_at"))
                    .to_owned(),
            )
            .await?;

        manager
            .alter_table(
                Table::alter()
                    .table(public_key_credential::Entity)
                    .drop_column(Alias::new("issued_by_warpgate"))
                    .to_owned(),
            )
            .await?;

        Ok(())
    }
}
