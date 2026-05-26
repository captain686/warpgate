use sea_orm_migration::prelude::*;

use crate::m00009_credential_models::{otp_credential, public_key_credential};

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m00050_target_scoped_ssh_credentials"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(public_key_credential::Entity)
                    .add_column_if_not_exists(ColumnDef::new(Alias::new("target_id")).uuid().null())
                    .to_owned(),
            )
            .await?;

        manager
            .alter_table(
                Table::alter()
                    .table(otp_credential::Entity)
                    .add_column_if_not_exists(ColumnDef::new(Alias::new("target_id")).uuid().null())
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .table(public_key_credential::Entity)
                    .name("credentials_public_key_user_target_idx")
                    .col(Alias::new("user_id"))
                    .col(Alias::new("target_id"))
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .table(otp_credential::Entity)
                    .name("credentials_otp_user_target_idx")
                    .col(Alias::new("user_id"))
                    .col(Alias::new("target_id"))
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_index(
                Index::drop()
                    .table(otp_credential::Entity)
                    .name("credentials_otp_user_target_idx")
                    .if_exists()
                    .to_owned(),
            )
            .await?;

        manager
            .drop_index(
                Index::drop()
                    .table(public_key_credential::Entity)
                    .name("credentials_public_key_user_target_idx")
                    .if_exists()
                    .to_owned(),
            )
            .await?;

        manager
            .alter_table(
                Table::alter()
                    .table(otp_credential::Entity)
                    .drop_column(Alias::new("target_id"))
                    .to_owned(),
            )
            .await?;

        manager
            .alter_table(
                Table::alter()
                    .table(public_key_credential::Entity)
                    .drop_column(Alias::new("target_id"))
                    .to_owned(),
            )
            .await?;

        Ok(())
    }
}
