use sea_orm_migration::prelude::*;

use crate::m00010_parameters::parameters;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m00049_ssh_client_auth_otp"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(parameters::Entity)
                    .add_column(
                        ColumnDef::new(Alias::new("ssh_client_auth_otp"))
                            .boolean()
                            .not_null()
                            .default(true),
                    )
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(parameters::Entity)
                    .drop_column(Alias::new("ssh_client_auth_otp"))
                    .to_owned(),
            )
            .await
    }
}
