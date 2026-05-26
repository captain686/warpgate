use sea_orm_migration::prelude::*;

use crate::m00010_parameters::parameters;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m00047_log_retention_strategy"
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
                        ColumnDef::new(Alias::new("log_retention_strategy"))
                            .string()
                            .not_null()
                            .default("max_age"),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .alter_table(
                Table::alter()
                    .table(parameters::Entity)
                    .add_column(
                        ColumnDef::new(Alias::new("log_max_size_megabytes"))
                            .big_integer()
                            .null(),
                    )
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(parameters::Entity)
                    .drop_column(Alias::new("log_max_size_megabytes"))
                    .to_owned(),
            )
            .await?;

        manager
            .alter_table(
                Table::alter()
                    .table(parameters::Entity)
                    .drop_column(Alias::new("log_retention_strategy"))
                    .to_owned(),
            )
            .await?;

        Ok(())
    }
}
